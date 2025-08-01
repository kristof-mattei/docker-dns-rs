use color_eyre::eyre;
#[cfg(not(target_os = "windows"))]
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinHandle;

pub mod env;

/// Use this when you have a `JoinHandle<Result<T, E>>`
/// and you want to use it with `tokio::try_join!`
/// when the task completes with an `Result::Err`
/// the `JoinHandle` itself will be `Result::Ok` and thus not
/// trigger the `tokio::try_join!`. This function flattens the 2:
/// `Result::Ok(T)` when both the join-handle AND
/// the result of the inner function are `Result::Ok`, and `Result::Err`
/// when either the join failed, or the inner task failed
#[expect(unused, reason = "Library Code")]
pub(crate) async fn flatten_handle<T, E>(
    handle: JoinHandle<Result<T, E>>,
) -> Result<T, eyre::Report>
where
    E: 'static + Sync + Send,
    eyre::Report: From<E>,
{
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err.into()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(not(target_os = "windows"))]
/// Waits forever for a sigterm
pub(crate) async fn wait_for_sigterm() -> Result<(), std::io::Error> {
    signal(SignalKind::terminate())?.recv().await;
    Ok(())
}
