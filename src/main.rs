use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use color_eyre::eyre::Report;
use hickory_server::proto::rr::Name;
use ipnet::IpNet;
use tokio::net::{TcpListener, UdpSocket};
use tokio::signal;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tracing::metadata::LevelFilter;
use tracing::{event, Level};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::dns_listener::{set_up_authority, set_up_catalog, set_up_dns_server};
use crate::docker::config::Config;
use crate::docker::daemon::Daemon;
use crate::docker::monitor::Monitor;
use crate::table::AuthorityWrapper;

mod dns_listener;
mod docker;
mod encoding;
mod env;
mod filters;
mod http_client;
mod models;
mod table;
mod utils;

struct DDArgs {
    domain: Name,
    records: Vec<(String, IpAddr)>,
    network_blacklist: Vec<IpNet>,
}

fn parse_args() -> Result<DDArgs, Report> {
    Ok(DDArgs {
        domain: String::from("docker.extension").parse()?,
        records: Vec::new(),
        network_blacklist: Vec::new(),
    })
}

fn main() -> Result<(), color_eyre::Report> {
    // set up .env
    // zenv::zenv!();

    color_eyre::config::HookBuilder::default()
        .capture_span_trace_by_default(false)
        .install()?;

    // set up logger
    // from_env defaults to RUST_LOG
    tracing_subscriber::registry()
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::DEBUG.into())
                .from_env_lossy(),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_error::ErrorLayer::default())
        .init();

    // initialize the runtime
    let rt = tokio::runtime::Runtime::new().unwrap();

    // start service
    rt.block_on(start_tasks())
}

async fn start_tasks() -> Result<(), color_eyre::Report> {
    let args = parse_args()?;

    // DNS
    let authority = Arc::new(set_up_authority(args.domain.clone()).await?);
    let authority_wrapper =
        AuthorityWrapper::new(authority.clone(), args.records, args.network_blacklist).await?;

    // docker
    let docker_config = Config::build()?;
    let docker = Arc::new(Daemon::new(docker_config));
    let docker_monitor = Monitor::new(docker.clone(), authority_wrapper, args.domain.clone());

    let token = CancellationToken::new();

    let (sender, receiver) = tokio::sync::mpsc::channel(50);

    let mut tasks = JoinSet::new();

    // event handler
    {
        let token = token.clone();
        tasks.spawn(async move {
            let _guard = token.clone().drop_guard();

            docker_monitor.consume_events(receiver, &token).await;

            event!(Level::INFO, "Event handler stopped");
        });
    }

    // pump messages from Docker to the DockerMonitor
    {
        let token = token.clone();
        tasks.spawn(async move {
            let _guard = token.clone().drop_guard();

            if let Err(e) = docker.produce_events(sender, &token).await {
                event!(Level::ERROR, ?e, "Event producer Handler failed");
            } else {
                event!(Level::INFO, "Event producer stopped");
            }
        });
    }

    {
        let token = token.clone();
        let socket = UdpSocket::bind("0.0.0.0:54000").await?;
        let listener = TcpListener::bind("0.0.0.0:54000").await?;

        tasks.spawn(async move {
            let _guard = token.clone().drop_guard();

            let catalog = set_up_catalog(args.domain, authority);
            set_up_dns_server(listener, socket, catalog, token).await;

            event!(Level::INFO, "DNS Server stopped");
        });
    }

    tokio::select! {
        // TODO ensure tasks are registered
        _ = utils::wait_for_sigterm() => {
            event!(Level::WARN, "Sigterm detected, stopping all tasks");
        },
        _ = signal::ctrl_c() => {
            event!(Level::WARN, "CTRL+C detected, stopping all tasks");
        },
        () = token.cancelled() => {
            event!(Level::ERROR, "Underlying task stopped, stopping all others tasks");
        },
    };

    // catch all cancel in case we got here via something else than a cancel token
    token.cancel();

    // wait for the tasks that holds the server to exit gracefully
    // this is easier to write than x separate timeoouts
    // while we don't know if any of them gets killed
    // this will do for now, and we can always trace back the logs
    if timeout(Duration::from_millis(10000), tasks.shutdown())
        .await
        .is_err()
    {
        event!(Level::ERROR, "Task didn't stop within allotted time!");
    }

    Ok(())
}
