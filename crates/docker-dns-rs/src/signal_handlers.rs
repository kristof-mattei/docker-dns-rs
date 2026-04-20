use color_eyre::eyre;
#[cfg(not(any(target_os = "windows", miri)))]
use tokio::signal::unix::SignalKind;
#[cfg(not(any(target_os = "windows", miri)))]
use tokio::signal::unix::signal;
use tracing::{Level, event};

use crate::shutdown::Shutdown;

#[expect(
    clippy::cast_possible_truncation,
    reason = "Waiting for `try_into()` to become const"
)]
const SIGINT: u8 = libc::SIGINT as u8;

#[expect(
    clippy::cast_possible_truncation,
    reason = "Waiting for `try_into()` to become const"
)]
const SIGTERM: u8 = libc::SIGTERM as u8;

async fn register_sigterm_handler() -> Result<(), std::io::Error> {
    #[cfg(not(any(target_os = "windows", miri)))]
    signal(SignalKind::terminate())?.recv().await;

    #[cfg(any(target_os = "windows", miri))]
    let _r = std::future::pending::<Result<(), std::io::Error>>().await;

    Ok(())
}

/// Waits forever for a `SIGTERM`.
pub async fn wait_for_sigterm() -> Shutdown {
    if let Err(error) = register_sigterm_handler().await {
        const MESSAGE: &str = "Failed to register SIGTERM handler";

        Shutdown::UnexpectedError(eyre::Report::from(error).wrap_err(MESSAGE))
    } else {
        event!(Level::WARN, "SIGTERM detected, stopping all tasks");

        Shutdown::Signal(SIGTERM)
    }
}

async fn register_sigint_handler() -> Result<(), std::io::Error> {
    #[cfg(not(miri))]
    tokio::signal::ctrl_c().await?;

    #[cfg(miri)]
    let _r = std::future::pending::<Result<(), std::io::Error>>().await;

    Ok(())
}

/// Waits forever for a `SIGINT`.
pub async fn wait_for_sigint() -> Shutdown {
    if let Err(error) = register_sigint_handler().await {
        const MESSAGE: &str = "Failed to register CTRL+c handler";

        Shutdown::UnexpectedError(eyre::Report::from(error).wrap_err(MESSAGE))
    } else {
        event!(Level::WARN, "CTRL+c detected, stopping all tasks");

        Shutdown::Signal(SIGINT)
    }
}
