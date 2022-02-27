//! Drives graceful shutdown when the process receives a signal.

use tokio::signal::unix::{signal, Signal, SignalKind};
use tracing::debug;

#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
pub use drain::Watch;

/// Drives shutdown by watching signals
#[derive(Debug)]
#[must_use = "call `Shutdown::on_signal` to await a signal"]
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
pub struct Shutdown {
    interrupt: Signal,
    terminate: Signal,
    tx: drain::Signal,
}

/// Indicates whether shutdown completed gracefully or was forced by a second signal
#[derive(Debug, thiserror::Error)]
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
#[error("process aborted by signal")]
pub struct Aborted(());

/// Indicates an error registering a signal handler
#[derive(Debug, thiserror::Error)]
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
#[error("failed to register signal handler: {0}")]
pub struct RegisterError(#[from] std::io::Error);

/// Creates a shutdown channel
///
/// [`Shutdown`] watches for `SIGINT` and `SIGTERM` signals. When a signal is received, [`Watch`]
/// instances are notifed and, when all watches are dropped, the shutdown is completed. If a second
/// signal is received while waiting for watches to be dropped, the shutdown is aborted.
///
/// If a second signal is received while waiting for shutdown to complete, the process
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
pub fn try_register() -> Result<(Shutdown, Watch), RegisterError> {
    let (drain_tx, drain_rx) = drain::channel();
    let shutdown = Shutdown::from_drain(drain_tx)?;
    Ok((shutdown, drain_rx))
}

impl Shutdown {
    fn from_drain(tx: drain::Signal) -> Result<Self, RegisterError> {
        let interrupt = signal(SignalKind::interrupt())?;
        let terminate = signal(SignalKind::terminate())?;
        Ok(Shutdown {
            interrupt,
            terminate,
            tx,
        })
    }

    /// Watches for signals and drives shutdown
    ///
    /// When a `SIGINT` or `SIGTERM` signal is received, the shutdown is initiated, notifying all
    /// [`Watch`] instances. When all watches are dropped, the shutdown is completed.
    ///
    /// If a second signal is received while waiting for watches to be dropped, this future
    /// completes immediately and [`Completion::Aborted`] is returned.
    ///
    /// An error is returned when signal registration fails.
    pub async fn signaled(self) -> Result<(), Aborted> {
        let Self {
            mut interrupt,
            mut terminate,
            tx,
        } = self;

        tokio::select! {
            _ = interrupt.recv() => {
                debug!("Received SIGINT; draining");
            },

            _ = terminate.recv() => {
                debug!("Received SIGTERM; draining");
            }
        }

        tokio::select! {
            _ = tx.drain() => {
                debug!("Drained");
                Ok(())
            },

            _ = interrupt.recv() => {
                debug!("Received SIGINT; aborting");
                Err(Aborted(()))
            },

            _ = terminate.recv() => {
                debug!("Received SIGTERM; aborting");
                Err(Aborted(()))
            }
        }
    }
}
