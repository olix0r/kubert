//! Drives graceful shutdown when the process receives a signal.

#[cfg(feature = "runtime")]
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tracing::debug;

#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
pub use drain::Watch;

#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
mod signals;

#[cfg(windows)]
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
use signals::windows::Signals;

#[cfg(unix)]
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
use signals::unix::Signals;

/// Drives shutdown by watching signals
#[derive(Debug)]
#[must_use = "call `Shutdown::on_signal` to await a signal"]
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
pub struct Shutdown {
    signals: Signals,
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

#[cfg(feature = "runtime")]
pin_project_lite::pin_project! {
    /// Indicates an error registering a signal handler
    #[cfg_attr(docsrs, doc(cfg(feature = "runtime")))]
    pub struct CancelOnShutdown<T> {
        #[pin]
        inner: T,
        #[pin]
        shutdown: Pin<Box<dyn std::future::Future<Output = ()> + Send + Sync + 'static>>,
    }
}

/// Creates a shutdown channel
///
/// [`Shutdown`] watches for `SIGINT` and `SIGTERM` signals. When a signal is received, [`Watch`]
/// instances are notifed and, when all watches are dropped, the shutdown is completed. If a second
/// signal is received while waiting for watches to be dropped, the shutdown is aborted.
///
/// If a second signal is received while waiting for shutdown to complete, the process
#[cfg(unix)]
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
#[deprecated(note = "please use `register` instead")]
pub fn sigint_or_sigterm() -> Result<(Shutdown, Watch), RegisterError> {
    register()
}

/// Creates a shutdown channel
///
/// [`Shutdown`] watches for `SIGINT` and `SIGTERM` signals on Linux or Ctrl-Shutdown on
/// Windows. When a signal is received, [`Watch`] instances are notifed and, when all watches are
/// dropped, the shutdown is completed. If a second signal is received while waiting for watches
/// to be dropped, the shutdown is aborted.
///
/// If a second signal is received while waiting for shutdown to complete, the process
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
pub fn register() -> Result<(Shutdown, Watch), RegisterError> {
    let signals = Signals::new()?;

    let (tx, rx) = drain::channel();
    let shutdown = Shutdown { signals, tx };
    Ok((shutdown, rx))
}

impl Shutdown {
    /// Watches for signals and drives shutdown
    ///
    /// When a signal is received, the shutdown is initiated, notifying all
    /// [`Watch`] instances. When all watches are dropped, the shutdown is completed.
    ///
    /// If a second signal is received while waiting for watches to be dropped, this future
    /// completes immediately with an [`Aborted`] error.
    pub async fn signaled(self) -> Result<(), Aborted> {
        let Self {
            mut signals,
            mut tx,
        } = self;

        tokio::select! {
            _ = signals.recv() => {
                debug!("draining");
            },

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

            _ = signals.recv() => {
                debug!("aborting");
                Err(Aborted(()))
            },
        }
    }
}

#[cfg(feature = "runtime")]
impl<T> CancelOnShutdown<T> {
    /// Wraps a `Future` or `Stream` that completes when the shutdown watch fires.
    ///
    /// The inner `Future`/`Stream` is given the chance to complete before the shutdown watch is
    /// polled so that it has a chance to complete its work before the task is cancelled.
    pub(crate) fn new(watch: Watch, inner: T) -> Self {
        // XXX Unfortunately the `Watch` API doesn't give us any means to poll for updates, so we
        // have to box the async call to poll it from the stream.
        let shutdown = Box::pin(async move {
            let _ = watch.signaled().await;
        });
        Self { inner, shutdown }
    }
}

#[cfg(feature = "runtime")]
impl<F: std::future::Future<Output = ()>> std::future::Future for CancelOnShutdown<F> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut this = self.project();

        // Drive the future to completion.
        if this.inner.poll(cx).is_ready() {
            return Poll::Ready(());
        }

        // If the future is pending, register interest in the shutdown watch and complete the future
        // if it has fired.
        this.shutdown.as_mut().poll(cx)
    }
}

#[cfg(feature = "runtime")]
impl<S: futures_core::Stream> futures_core::Stream for CancelOnShutdown<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        use std::future::Future;

        let mut this = self.project();

        // Process items from the stream until it is pending (or the stream ends).
        if let Poll::Ready(next) = this.inner.poll_next(cx) {
            return Poll::Ready(next);
        }

        // If the stream is pending, register interest in the shutdown watch and end the stream if
        // it has fired.
        if this.shutdown.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }

        Poll::Pending
    }
}

#[cfg(all(test, feature = "runtime"))]
mod test {
    use super::CancelOnShutdown;
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_test::{assert_pending, assert_ready, assert_ready_eq, task};

    #[tokio::test]
    async fn cancel_stream_drains() {
        let (shutdown_tx, shutdown_rx) = drain::channel();

        let (stream_tx, stream_rx) = tokio::sync::mpsc::channel(3);
        let mut stream_rx = task::spawn(CancelOnShutdown::new(
            shutdown_rx,
            ReceiverStream::new(stream_rx),
        ));
        stream_tx.try_send(1).unwrap();
        stream_tx.try_send(2).unwrap();
        stream_tx.try_send(3).unwrap();

        assert_ready_eq!(stream_rx.poll_next(), Some(1));

        let mut drain = task::spawn(shutdown_tx.drain());
        assert_ready_eq!(stream_rx.poll_next(), Some(2));
        assert_ready_eq!(stream_rx.poll_next(), Some(3));
        assert_pending!(drain.poll());
        assert_ready_eq!(stream_rx.poll_next(), None);
        assert_ready!(drain.poll());
    }

    #[tokio::test]
    async fn cancel_future_ends() {
        let (shutdown_tx, shutdown_rx) = drain::channel();

        let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
        let mut rx = task::spawn(CancelOnShutdown::new(
            shutdown_rx,
            Box::pin(async move {
                rx.await.unwrap();
            }),
        ));
        assert_pending!(rx.poll());

        let mut drain = task::spawn(shutdown_tx.drain());
        assert_pending!(drain.poll());
        assert_ready!(rx.poll());
        assert_ready!(drain.poll());
    }
}
