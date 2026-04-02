use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hashbrown::HashMap;
use hickory_server::ServerFuture;
use hickory_server::authority::{Catalog, MessageResponseBuilder};
use hickory_server::proto::ProtoError;
use hickory_server::proto::op::{Header, ResponseCode};
use hickory_server::proto::rr::rdata::{PTR, SOA};
use hickory_server::proto::rr::{LowerName, Name, RData, Record, RecordSet, RecordType, RrKey};
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use hickory_server::store::in_memory::InMemoryAuthority;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use crate::args::RawDomainIntercept;

pub struct DnsRequestHandler {
    catalog: Arc<RwLock<Catalog>>,
    intercepts: HashMap<LowerName, Vec<RData>>,
}

impl DnsRequestHandler {
    pub fn new(catalog: Arc<RwLock<Catalog>>, intercepts: Vec<RawDomainIntercept>) -> Self {
        let mut map: HashMap<LowerName, Vec<RData>> = HashMap::new();

        for intercept in intercepts {
            // Forward: A or AAAA
            map.entry(LowerName::from(&intercept.name))
                .or_default()
                .push(RData::from(intercept.addr));

            // Reverse: PTR
            let ptr_name = Name::from(intercept.addr);
            map.entry(LowerName::from(&ptr_name))
                .or_default()
                .push(RData::PTR(PTR(intercept.name)));
        }
        Self {
            catalog,
            intercepts: map,
        }
    }
}

#[async_trait]
impl RequestHandler for DnsRequestHandler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        if let Ok(request_info) = request.request_info() {
            let qname = request_info.query.name();
            let qtype = request_info.query.query_type();

            if let Some(rdatas) = self.intercepts.get(qname) {
                let answers: Vec<Record> = rdatas
                    .iter()
                    .filter(|rdata| qtype == rdata.record_type() || qtype == RecordType::ANY)
                    .map(|rdata| Record::from_rdata(Name::from(qname.clone()), 0, rdata.clone()))
                    .collect();

                event!(Level::DEBUG, %qname, %qtype, answers = answers.len(), "DNS intercept matched");

                let builder = MessageResponseBuilder::from_message_request(request);
                let mut header = Header::response_from_request(request_info.header);
                header.set_authoritative(true);

                let response = builder.build(
                    header,
                    answers.iter(),
                    std::iter::empty(),
                    std::iter::empty(),
                    std::iter::empty(),
                );

                return match response_handle.send_response(response).await {
                    Ok(info) => info,
                    Err(error) => {
                        event!(
                            Level::ERROR,
                            ?error,
                            "failed to send intercept DNS response"
                        );
                        let mut error_header = Header::response_from_request(request_info.header);
                        error_header.set_response_code(ResponseCode::ServFail);
                        ResponseInfo::from(error_header)
                    },
                };
            }
        }

        // fall back the catalog which contain the dynamically registered containers
        self.catalog
            .read()
            .await
            .handle_request(request, response_handle)
            .await
    }
}

pub async fn set_up_dns_server<H>(
    tcp_listener: TcpListener,
    udp_socket: UdpSocket,
    handler: H,
    cancellation_token: CancellationToken,
) where
    H: RequestHandler,
{
    let mut dns_listener = ServerFuture::new(handler);

    dns_listener.register_socket(udp_socket);
    dns_listener.register_listener(tcp_listener, Duration::from_secs(1));

    tokio::select! {
           r = dns_listener.block_until_done() => {
               event!(Level::INFO, "DNS Server ended");

               handle_server_shutdown(r);
           },
           () = cancellation_token.cancelled() => {
               event!(Level::INFO, "DNS Server cancelled externally");

               handle_server_shutdown(dns_listener.shutdown_gracefully().await);

           }
    }
}

pub async fn set_up_authority(domain: Name) -> Result<InMemoryAuthority, color_eyre::Report> {
    let tree = BTreeMap::<RrKey, RecordSet>::from([(
        RrKey::new(
            domain.clone().into(),
            hickory_server::proto::rr::RecordType::SOA,
        ),
        Record::from_rdata(
            domain.clone(),
            3600,
            RData::SOA(SOA::new(domain.clone(), domain.clone(), 0, 0, 0, 0, 0)),
        )
        .into(),
    )]);

    let imo = InMemoryAuthority::new(
        domain,
        tree,
        hickory_server::authority::ZoneType::Primary,
        false,
    )
    .map_err(color_eyre::Report::msg)?;

    Ok(imo)
}

pub fn set_up_catalog<I: Into<LowerName>>(domain: I, authority: Arc<InMemoryAuthority>) -> Catalog {
    let mut catalog = Catalog::new();

    catalog.upsert(domain.into(), vec![authority]);

    catalog
}

fn handle_server_shutdown(server_shutdown_result: Result<(), ProtoError>) {
    if let Err(error) = server_shutdown_result {
        event!(
            Level::ERROR,
            ?error,
            "Requested graceful shutdown, did not happen"
        );
    } else {
        event!(Level::INFO, "DNS server shut down gracefully");
    }
}
