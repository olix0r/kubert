//! Drives graceful shutdown when the process receives a signal.

use futures_core::{Future, Stream};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
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

pin_project_lite::pin_project! {
    /// Indicates an error registering a signal handler
    #[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
    pub struct CancelOnShutdown<T> {
        #[pin]
        inner: T,
        #[pin]
        shutdown: Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>,
    }
}

/// Creates a shutdown channel
///
/// [`Shutdown`] watches for `SIGINT` and `SIGTERM` signals. When a signal is received, [`Watch`]
/// instances are notifed and, when all watches are dropped, the shutdown is completed. If a second
/// signal is received while waiting for watches to be dropped, the shutdown is aborted.
///
/// If a second signal is received while waiting for shutdown to complete, the process
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
pub fn sigint_or_sigterm() -> Result<(Shutdown, Watch), RegisterError> {
    let interrupt = signal(SignalKind::interrupt())?;
    let terminate = signal(SignalKind::terminate())?;

    let (tx, rx) = drain::channel();
    let shutdown = Shutdown {
        interrupt,
        terminate,
        tx,
    };
    Ok((shutdown, rx))
}

impl Shutdown {
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
            mut tx,
        } = self;

        tokio::select! {
            _ = interrupt.recv() => {
                debug!("Received SIGINT; draining");
            },

            _ = terminate.recv() => {
                debug!("Received SIGTERM; draining");
            }

            _ = tx.closed() => {
                debug!("All shutdown receivers dropped");
                // Drain can't do anything if the receivers have been dropped
                return Ok(());
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

impl<T> CancelOnShutdown<T> {
    /// Wraps a `Future` or `Stream` that completes when the shutdown watch fires.
    pub(crate) fn new(watch: Watch, inner: T) -> Self {
        // XXX Unfortunately the `Watch` API doesn't give us any means to poll for updates, so we
        // have to box the async call to poll it from the stream.
        let shutdown = Box::pin(async move {
            let _ = watch.signaled().await;
        });
        Self { inner, shutdown }
    }
}

impl<F: Future<Output = ()>> Future for CancelOnShutdown<F> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut this = self.project();

        if this.shutdown.as_mut().poll(cx).is_ready() {
            return Poll::Ready(());
        }

        this.inner.poll(cx)
    }
}

impl<S: Stream> Stream for CancelOnShutdown<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let mut this = self.project();

        if this.shutdown.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }

        this.inner.poll_next(cx)
    }
}
