use std::env;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use color_eyre::eyre;
use tokio::net::{TcpListener, UdpSocket};
use tokio::signal;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Level, event};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::dns_listener::{set_up_authority, set_up_catalog, set_up_dns_server};
use crate::docker::config::Config;
use crate::docker::daemon::Daemon;
use crate::docker::monitor::Monitor;
use crate::table::AuthorityWrapper;

mod args;
mod dns_listener;
mod docker;
mod encoding;
mod filters;
mod http_client;
mod models;
mod table;
mod utils;

fn init_tracing() -> Result<(), eyre::Report> {
    let main_filter = EnvFilter::builder().parse(
        env::var(EnvFilter::DEFAULT_ENV)
            .unwrap_or_else(|_| format!("INFO,{}=TRACE", env!("CARGO_CRATE_NAME"))),
    )?;

    let layers = vec![
        #[cfg(feature = "tokio-console")]
        console_subscriber::ConsoleLayer::builder()
            .with_default_env()
            .spawn()
            .boxed(),
        tracing_subscriber::fmt::layer()
            .with_filter(main_filter)
            .boxed(),
        tracing_error::ErrorLayer::default().boxed(),
    ];

    Ok(tracing_subscriber::registry().with(layers).try_init()?)
}

fn main() -> Result<(), color_eyre::Report> {
    // set up .env
    // zenv::zenv!();

    color_eyre::config::HookBuilder::default()
        .capture_span_trace_by_default(false)
        .install()?;

    // set up logger
    init_tracing()?;

    // initialize the runtime
    let rt = tokio::runtime::Runtime::new().unwrap();

    // start service
    rt.block_on(start_tasks())
}

async fn start_tasks() -> Result<(), color_eyre::Report> {
    let args = args::Args::parse();

    // DNS
    let authority = Arc::new(set_up_authority(args.domain.clone()).await?);
    let authority_wrapper = AuthorityWrapper::new(Arc::clone(&authority));

    // docker
    let docker_config = Config::build()?;
    let docker = Arc::new(Daemon::new(docker_config));
    let docker_monitor = Monitor::new(Arc::clone(&docker), authority_wrapper, args.domain.clone());

    let token = CancellationToken::new();

    let (sender, receiver) = tokio::sync::mpsc::channel(50);

    let tasks = TaskTracker::new();

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

    #[cfg(not(target_os = "windows"))]
    let sigterm = utils::wait_for_sigterm();

    #[cfg(target_os = "windows")]
    let sigterm = std::future::pending::<Result<(), std::io::Error>>();

    tokio::select! {
        // TODO ensure tasks are registered
        _ = sigterm => {
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

    tasks.close();

    // wait for the tasks that holds the server to exit gracefully
    // this is easier to write than x separate timeoouts
    // while we don't know if any of them gets killed
    // this will do for now, and we can always trace back the logs
    if timeout(Duration::from_millis(10000), tasks.wait())
        .await
        .is_err()
    {
        event!(Level::ERROR, "Task didn't stop within allotted time!");
    }

    Ok(())
}
