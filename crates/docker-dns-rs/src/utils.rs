use tokio::task::JoinHandle;

use crate::shutdown::Shutdown;

pub mod task;

pub async fn flatten_shutdown_handle(
    handle: JoinHandle<Result<!, Shutdown>>,
) -> Result<!, Shutdown> {
    handle.await?
}
