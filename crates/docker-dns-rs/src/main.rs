use std::convert::Infallible;
use std::env::{self, VarError};
use std::process::{ExitCode, Termination as _};
use std::sync::Arc;
use std::time::Duration;

use color_eyre::config::HookBuilder;
use color_eyre::eyre;
use dotenvy::dotenv;
use hickory_server::zone_handler::Catalog;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Level, event};
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use twistlock::client::Client as Daemon;
use twistlock::models::events::Event;

use crate::build_env::get_build_env;
use crate::config::{AppConfig, RawRecord};
use crate::dns_listener::{DnsRequestHandler, set_up_authority, set_up_catalog, set_up_dns_server};
use crate::docker::monitor::Monitor;
use crate::shutdown::Shutdown;
use crate::table::AuthorityWrapper;
use crate::task_tracker_ext::TaskTrackerExt as _;
use crate::utils::flatten_shutdown_handle;
use crate::utils::task::spawn_with_name;

mod build_env;
mod config;
mod dns_listener;
mod docker;
mod shutdown;
mod signal_handlers;
mod table;
mod task_tracker_ext;
mod utils;

#[cfg_attr(not(miri), global_allocator)]
#[cfg_attr(miri, expect(unused, reason = "Not supported in Miri"))]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn build_filter() -> (EnvFilter, Option<eyre::Report>) {
    fn build_default_filter() -> EnvFilter {
        EnvFilter::builder()
            .parse(format!("INFO,{}=TRACE", env!("CARGO_CRATE_NAME")))
            .expect("Default filter should always work")
    }

    let (filter, parsing_error) = match env::var(EnvFilter::DEFAULT_ENV) {
        Ok(user_directive) => match EnvFilter::builder().parse(user_directive) {
            Ok(filter) => (filter, None),
            Err(error) => (build_default_filter(), Some(eyre::Report::new(error))),
        },
        Err(VarError::NotPresent) => (build_default_filter(), None),
        Err(error @ VarError::NotUnicode(_)) => {
            (build_default_filter(), Some(eyre::Report::new(error)))
        },
    };

    (filter, parsing_error)
}

fn init_tracing(filter: EnvFilter) -> Result<(), eyre::Report> {
    let registry = tracing_subscriber::registry();

    #[cfg(feature = "tokio-console")]
    let registry = registry.with(console_subscriber::ConsoleLayer::builder().spawn());

    Ok(registry
        .with(tracing_subscriber::fmt::layer().with_filter(filter))
        .with(tracing_error::ErrorLayer::default())
        .try_init()?)
}

fn main() -> ExitCode {
    // set up .env, if it fails, user didn't provide any
    let _r = dotenv();

    HookBuilder::default()
        .capture_span_trace_by_default(true)
        .display_env_section(false)
        .install()
        .expect("Failed to install panic handler");

    let (env_filter, parsing_error) = build_filter();

    init_tracing(env_filter).expect("Failed to set up tracing");

    // bubble up the parsing error
    if let Err(error) = parsing_error.map_or(Ok(()), Err) {
        return Err::<Infallible, _>(error).report();
    }

    // initialize the runtime
    let shutdown: Shutdown = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("Failed building the Runtime")
        .block_on(async {
            // explicitly launch everything in a spawned task
            // see https://docs.rs/tokio/latest/tokio/attr.main.html#non-worker-async-function
            let handle = spawn_with_name("main task runner", start_tasks());

            flatten_shutdown_handle(handle).await
        });

    shutdown.report()
}

fn print_header() {
    const NAME: &str = env!("CARGO_PKG_NAME");
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    let build_env = get_build_env();

    event!(
        Level::INFO,
        "{} v{} - built for {} ({})",
        NAME,
        VERSION,
        build_env.get_target(),
        build_env.get_target_cpu().unwrap_or("base cpu variant"),
    );
}

// This function would be shorter if we had `FromResidual`
async fn start_tasks() -> Shutdown {
    print_header();

    let AppConfig {
        docker_config,
        domain,
        dns_bind,
        records,
    } = match AppConfig::build() {
        Ok(config) => config,
        Err(error) => return Shutdown::from(error),
    };

    // DNS
    let authority = match set_up_authority(domain.clone()).await {
        Ok(authority) => authority,
        Err(error) => return Shutdown::from(error),
    };

    let forward_authority = Arc::new(authority);
    let catalog = Arc::new(tokio::sync::RwLock::new(set_up_catalog(
        domain.clone(),
        Arc::clone(&forward_authority),
    )));
    let authority_wrapper = AuthorityWrapper::new(Arc::clone(&forward_authority));

    // docker
    let daemon = match Daemon::build(
        docker_config.docker_host,
        docker_config.cacert,
        docker_config.client_key,
        docker_config.client_cert,
        docker_config.timeout,
    ) {
        Ok(daemon) => daemon,
        Err(error) => return Shutdown::from(error),
    };

    let docker = Arc::new(daemon);

    let docker_monitor = Monitor::new(
        Arc::clone(&docker),
        authority_wrapper,
        Arc::clone(&catalog),
        domain.clone(),
    );

    let cancellation_token = CancellationToken::new();

    let (sender, receiver) = tokio::sync::mpsc::channel(50);

    let tasks = TaskTracker::new();

    // event handler
    {
        tasks.spawn_with_name(
            "docker event monitor",
            docker_event_monitor(docker_monitor, receiver, cancellation_token.clone()),
        );
    }

    // pump messages from Docker to the DockerMonitor
    {
        tasks.spawn_with_name(
            "docker listener",
            docker_listener(docker, sender, cancellation_token.clone()),
        );
    }

    {
        let socket = match UdpSocket::bind(dns_bind).await {
            Ok(socket) => socket,
            Err(error) => return Shutdown::from(error),
        };

        let listener = match TcpListener::bind(dns_bind).await {
            Ok(listener) => listener,
            Err(error) => return Shutdown::from(error),
        };

        tasks.spawn_with_name(
            "dns handler",
            dns_handler(
                socket,
                listener,
                Arc::clone(&catalog),
                records,
                cancellation_token.clone(),
            ),
        );
    }

    // now we wait forever for either
    // * SIGTERM
    // * CTRL+c (SIGINT)
    // * cancellation of the shutdown token, triggered by another task when it
    //   completes unexpectedly (which means it failed)
    let shutdown_reason = tokio::select! {
        biased;
        () = cancellation_token.cancelled() => {
            event!(Level::WARN, "Underlying task stopped, stopping all other tasks");

            Shutdown::OperationalFailure {
                code: ExitCode::FAILURE,
                message: "Some task unexpectedly failed which triggered a shutdown."
            }
        },
        result = signal_handlers::wait_for_sigterm() => {
            result
        },
        result = signal_handlers::wait_for_sigint() => {
            result
        },
    };

    // catch all cancel in case we got here via something else than a cancellation token
    cancellation_token.cancel();

    tasks.close();

    // wait for the tasks that holds the server to exit gracefully
    // this is easier to write than x separate timeoouts
    // while we don't know if any of them gets killed
    // this will do for now, and we can always trace back the logs
    if timeout(Duration::from_secs(10), tasks.wait())
        .await
        .is_err()
    {
        event!(Level::ERROR, "Task didn't stop within allotted time!");
    }

    shutdown_reason
}

async fn dns_handler(
    socket: UdpSocket,
    listener: TcpListener,
    catalog: Arc<RwLock<Catalog>>,
    records: Vec<RawRecord>,
    cancellation_token: CancellationToken,
) {
    let _guard = cancellation_token.clone().drop_guard();

    let handler = DnsRequestHandler::new(catalog, records);
    set_up_dns_server(listener, socket, handler, cancellation_token).await;

    event!(Level::INFO, "DNS Server stopped");
}

async fn docker_listener(
    docker: Arc<Daemon>,
    sender: tokio::sync::mpsc::Sender<Event>,
    cancellation_token: CancellationToken,
) {
    let _guard = cancellation_token.clone().drop_guard();

    if let Err(error) = docker.produce_events(sender, &cancellation_token).await {
        event!(Level::ERROR, ?error, "Event producer Handler failed");
    } else {
        event!(Level::INFO, "Event producer stopped");
    }
}

async fn docker_event_monitor(
    docker_monitor: Monitor,
    receiver: Receiver<Event>,
    cancellation_token: CancellationToken,
) {
    let _guard = cancellation_token.clone().drop_guard();

    if let Err(error) = docker_monitor.start().await {
        event!(Level::ERROR, ?error, "Failed to fetch containers");
        return;
    }

    docker_monitor
        .consume_events(receiver, &cancellation_token)
        .await;

    event!(Level::INFO, "Event handler stopped");
}
