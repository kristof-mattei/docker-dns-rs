use std::sync::Arc;
use std::time::Duration;

use hickory_server::authority::Catalog;
use hickory_server::proto::error::ProtoError;
use hickory_server::proto::rr::rdata::SOA;
use hickory_server::proto::rr::{RData, Record};
use hickory_server::store::in_memory::InMemoryAuthority;
use hickory_server::ServerFuture;
use tokio::net::{TcpListener, UdpSocket};
use tokio_util::sync::CancellationToken;
use tracing::{event, Level};

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
    };
}

pub async fn set_up_authority(domain: &str) -> Result<InMemoryAuthority, color_eyre::Report> {
    let imo = InMemoryAuthority::empty(
        domain.parse()?,
        hickory_server::authority::ZoneType::Primary,
        false,
    );

    imo.upsert(
        Record::from_rdata(
            domain.parse()?,
            0,
            RData::SOA(SOA::new(
                domain.parse()?,
                "root.docker".parse()?,
                0,
                0,
                0,
                0,
                0,
            )),
        ),
        0,
    )
    .await;

    Ok(imo)
}

pub fn set_up_catalog(
    domain: &str,
    authority: Arc<InMemoryAuthority>,
) -> Result<Catalog, color_eyre::Report> {
    let mut catalog = Catalog::new();

    catalog.upsert(domain.parse()?, Box::new(authority));

    Ok(catalog)
}

fn handle_server_shutdown(server_shutdown_result: Result<(), ProtoError>) {
    if let Err(e) = server_shutdown_result {
        event!(
            Level::ERROR,
            ?e,
            "Requested graceful shutdown, did not happen"
        );
    } else {
        event!(Level::INFO, "DNS server shut down gracefully");
    }
}
