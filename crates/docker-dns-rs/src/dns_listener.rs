use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use color_eyre::eyre;
use hashbrown::{HashMap, HashSet};
use hickory_net::NetError;
use hickory_net::proto::op::{HeaderCounts, Metadata};
use hickory_net::runtime::{Time, TokioTime};
use hickory_server::Server;
use hickory_server::proto::op::{Header, ResponseCode};
use hickory_server::proto::rr::rdata::{PTR, SOA};
use hickory_server::proto::rr::{LowerName, Name, RData, Record, RecordSet, RecordType, RrKey};
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use hickory_server::store::in_memory::InMemoryZoneHandler;
use hickory_server::zone_handler::{AxfrPolicy, Catalog, MessageResponseBuilder, ZoneType};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use crate::config::RawRecord;

#[derive(Clone, Debug, PartialEq, Eq)]
struct HashedRData(RData);

impl Hash for HashedRData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.record_type().hash(state);

        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "We're only interested in A, AAAA and PTR"
        )]
        match &self.0 {
            &RData::A(ref a) => a.hash(state),
            &RData::AAAA(ref aaaa) => aaaa.hash(state),
            &RData::PTR(ref ptr) => ptr.hash(state),
            other => unreachable!("unexpected RData variant in intercept map: {:?}", other),
        }
    }
}

pub struct DnsRequestHandler {
    catalog: Arc<RwLock<Catalog>>,
    intercepts: HashMap<LowerName, HashSet<HashedRData>>,
}

impl DnsRequestHandler {
    pub fn new(catalog: Arc<RwLock<Catalog>>, intercepts: Vec<RawRecord>) -> Self {
        let mut map: HashMap<LowerName, HashSet<HashedRData>> = HashMap::new();

        for intercept in intercepts {
            // Forward: A or AAAA
            let forward = HashedRData(RData::from(intercept.addr));
            if !map
                .entry(LowerName::from(&intercept.name))
                .or_default()
                .insert(forward)
            {
                event!(
                    Level::WARN,
                    name = %intercept.name,
                    addr = %intercept.addr,
                    "duplicate --record entry ignored"
                );
            }

            // Reverse: PTR
            let ptr_name = Name::from(intercept.addr);
            let ptr = HashedRData(RData::PTR(PTR(intercept.name)));
            map.entry(LowerName::from(&ptr_name))
                .or_default()
                .insert(ptr);
        }
        Self {
            catalog,
            intercepts: map,
        }
    }
}

#[async_trait]
impl RequestHandler for DnsRequestHandler {
    async fn handle_request<R: ResponseHandler, T: Time>(
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
                    .filter(|r| qtype == r.0.record_type() || qtype == RecordType::ANY)
                    .map(|r| Record::from_rdata(Name::from(qname.clone()), 5, r.0.clone()))
                    .collect();

                event!(Level::TRACE, %qname, %qtype, answers = answers.len(), "DNS intercept match");

                let builder = MessageResponseBuilder::from_message_request(request);
                let mut metadata = Metadata::response_from_request(request_info.metadata);
                metadata.authoritative = true;

                let response = builder.build(
                    metadata,
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
                            "failed to send intercepted DNS response"
                        );

                        let mut error_metadata =
                            Metadata::response_from_request(request_info.metadata);
                        error_metadata.response_code = ResponseCode::ServFail;

                        let header = Header {
                            metadata: error_metadata,
                            counts: HeaderCounts::default(),
                        };

                        ResponseInfo::from(header)
                    },
                };
            }
        }

        // fall back to the catalog that contains the dynamically registered containers
        self.catalog
            .read()
            .await
            .handle_request::<_, TokioTime>(request, response_handle)
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
    // see https://github.com/hickory-dns/hickory-dns/blob/7a5678135c48f5bfc212824bc369634901ba4bc6/bin/src/config/mod.rs#L236-L251
    const RESPONSE_BUFFER_SIZE: usize = 32;

    let mut dns_listener = Server::new(handler);

    dns_listener.register_socket(udp_socket);
    dns_listener.register_listener(tcp_listener, Duration::from_secs(1), RESPONSE_BUFFER_SIZE);

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

pub async fn set_up_authority(domain: Name) -> Result<InMemoryZoneHandler, eyre::Report> {
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

    let imo = InMemoryZoneHandler::new(domain, tree, ZoneType::Primary, AxfrPolicy::Deny)
        .map_err(eyre::Report::msg)?;

    Ok(imo)
}

pub fn set_up_catalog<I: Into<LowerName>>(
    domain: I,
    authority: Arc<InMemoryZoneHandler>,
) -> Catalog {
    let mut catalog = Catalog::new();

    catalog.upsert(domain.into(), vec![authority]);

    catalog
}

fn handle_server_shutdown(server_shutdown_result: Result<(), NetError>) {
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
