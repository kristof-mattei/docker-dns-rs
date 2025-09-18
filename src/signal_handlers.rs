#[cfg(not(target_os = "windows"))]
use tokio::signal::unix::{SignalKind, signal};

/// Waits forever for a SIGTERM
pub async fn wait_for_sigterm() -> Result<(), std::io::Error> {
    #[cfg(not(target_os = "windows"))]
    signal(SignalKind::terminate())?.recv().await;

    #[cfg(target_os = "windows")]
    let _f = std::future::pending::<Result<(), std::io::Error>>().await;

    Ok(())
}

/// Waits forever for a SIGINT
pub async fn wait_for_sigint() -> Result<(), std::io::Error> {
    tokio::signal::ctrl_c().await?;

    Ok(())
}
