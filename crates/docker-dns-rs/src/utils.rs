use tokio::task::JoinHandle;

use crate::shutdown::Shutdown;

pub(crate) mod task;

pub(crate) async fn flatten_shutdown_handle(handle: JoinHandle<Shutdown>) -> Shutdown {
    match handle.await {
        Ok(shutdown) => shutdown,
        Err(join_error) => Shutdown::UnexpectedError(join_error.into()),
    }
}
