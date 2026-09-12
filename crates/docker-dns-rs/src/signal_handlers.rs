#[cfg(not(target_os = "windows"))]
use std::io::Error;

use color_eyre::eyre;
#[cfg(not(target_os = "windows"))]
use libc::c_int;
#[cfg(not(any(target_os = "windows", miri)))]
use tokio::signal::unix::SignalKind;
#[cfg(not(any(target_os = "windows", miri)))]
use tokio::signal::unix::signal;
use tracing::{Level, event};

use crate::shutdown::Shutdown;

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "Waiting for `try_into()` to become const"
)]
const SIGINT: u8 = libc::SIGINT as u8;

#[expect(
    clippy::as_conversions,
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

/// Sets signal back to its default action and raises it, killing this process.
/// Returns when the raise did not terminate the process: PID 1 of a PID namespace only receives signals it has a handler for, and the reset removes it.
#[cfg(not(target_os = "windows"))]
pub fn terminate_by_signal(signal: u8) {
    let signum = c_int::from(signal);

    // tokio's handler stays installed for the rest of the process (`Signal`'s caveats), so without this reset the raise runs it instead
    // SAFETY: `signal(2)` with `SIG_DFL` has no preconditions
    if unsafe { libc::signal(signum, libc::SIG_DFL) } == libc::SIG_ERR {
        event!(
            Level::ERROR,
            error = %Error::last_os_error(),
            signal,
            "Failed to restore the default signal disposition"
        );

        return;
    }

    // SAFETY: `raise(3)` has no preconditions
    if unsafe { libc::raise(signum) } != 0 {
        event!(
            Level::ERROR,
            error = %Error::last_os_error(),
            signal,
            "Failed to raise the signal"
        );
    }
}

/// Never called on Windows: the waits above never resolve there, so `Shutdown::Signal` is never constructed.
#[cfg(target_os = "windows")]
pub fn terminate_by_signal(_signal: u8) {}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    mod unix {
        use std::os::unix::process::ExitStatusExt as _;
        use std::process::{Command, Stdio};

        use pretty_assertions::assert_eq;
        use tokio::signal::unix::{SignalKind, signal};

        use crate::signal_handlers::{SIGTERM, terminate_by_signal};

        const CHILD_MARKER: &str = "DOCKER_DNS_RS_TERMINATE_BY_SIGNAL_CHILD";

        // the raise kills the calling process, so the scenario runs in a re-executed copy of this test binary
        #[test]
        fn dies_by_the_raised_signal_despite_tokio_handler() {
            if std::env::var_os(CHILD_MARKER).is_some() {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .unwrap();

                // installs tokio's handler, the case under test
                runtime.block_on(async {
                    let _listener = signal(SignalKind::terminate()).unwrap();
                });

                drop(runtime);

                terminate_by_signal(SIGTERM);

                // surviving the raise exits 0, which fails the parent's assertion
                return;
            }

            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "signal_handlers::tests::unix::dies_by_the_raised_signal_despite_tokio_handler",
                ])
                .env(CHILD_MARKER, "1")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();

            assert_eq!(status.signal(), Some(libc::SIGTERM));
        }
    }
}
