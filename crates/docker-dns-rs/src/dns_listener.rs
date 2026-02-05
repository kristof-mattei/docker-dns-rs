use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use hickory_server::ServerFuture;
use hickory_server::authority::Catalog;
use hickory_server::proto::ProtoError;
use hickory_server::proto::rr::rdata::SOA;
use hickory_server::proto::rr::{LowerName, Name, RData, Record, RecordSet, RrKey};
use hickory_server::store::in_memory::InMemoryAuthority;
use tokio::net::{TcpListener, UdpSocket};
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

pub async fn set_up_dns_server(
    tcp_listener: TcpListener,
    udp_socket: UdpSocket,
    catalog: Catalog,
    token: CancellationToken,
) {
    let mut dns_listener = ServerFuture::new(catalog);

    dns_listener.register_socket(udp_socket);
    dns_listener.register_listener(tcp_listener, Duration::from_secs(1));

    tokio::select! {
           r = dns_listener.block_until_done() => {
               event!(Level::INFO, "DNS Server ended");

               handle_server_shutdown(r);
           },
           () = token.cancelled() => {
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
