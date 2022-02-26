//! A utility for waiting for components to be initialized.

use futures_core::{Future, Stream};
use futures_util::ready;
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Tracks process initialization
///
/// Grants handles to components that need to be initialized and then waits for all handles to be
/// dropped to signal readiness.
#[derive(Debug)]
pub struct Initialized {
    semaphore: Arc<Semaphore>,
    issued: u32,
}

/// Signals a component has been initialized
#[derive(Debug)]
#[must_use]
pub struct Handle(OwnedSemaphorePermit);

pin_project_lite::pin_project! {
    /// A wrapper that releases a `Handle` when the underlying `Future` or `Stream` becomes ready
    #[derive(Debug)]
    pub struct ReleasesOnReady<T> {
        #[pin]
        inner: T,
        handle: Option<Handle>,
    }
}

impl Default for Initialized {
    fn default() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(0)),
            issued: 0,
        }
    }
}

impl Initialized {
    /// Creates a new [`Handle`] for a component to be dropped when the component has been
    /// initialized
    pub fn add_handle(&mut self) -> Handle {
        let sem = self.semaphore.clone();
        sem.add_permits(1);
        let permit = sem
            .try_acquire_owned()
            .expect("semaphore must issue permit");
        self.issued += 1;
        Handle(permit)
    }

    /// Waits for all handles to be dropped
    pub async fn initialized(self) {
        let _permit = self
            .semaphore
            .acquire_many(self.issued)
            .await
            .expect("semaphore cannot be closed");
    }
}

impl<T> ReleasesOnReady<T> {
    /// Wraps `T` so that the [`Handle`] is dropped when `T` is ready
    pub fn new(inner: T, handle: Handle) -> Self {
        Self {
            inner,
            handle: Some(handle),
        }
    }
}

impl<F: Future> Future for ReleasesOnReady<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let mut this = self.project();
        let out = ready!(this.inner.as_mut().poll(cx));
        drop(this.handle.take());
        Poll::Ready(out)
    }
}

impl<S: Stream> Stream for ReleasesOnReady<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let mut this = self.project();
        let next = ready!(this.inner.as_mut().poll_next(cx));
        drop(this.handle.take());
        Poll::Ready(next)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_test::{assert_pending, assert_ready, task};

    #[tokio::test]
    async fn initializes() {
        let mut init = task::spawn(Initialized::default().initialized());
        assert_ready!(init.poll());
    }

    #[tokio::test]
    async fn initializes_on_drop() {
        let mut init = Initialized::default();
        let handle0 = init.add_handle();
        let handle1 = init.add_handle();
        let mut init = task::spawn(init.initialized());
        assert_pending!(init.poll());
        drop(handle0);
        assert_pending!(init.poll());
        drop(handle1);
        assert_ready!(init.poll());
    }

    #[tokio::test]
    async fn initializes_on_future() {
        let mut init = Initialized::default();
        let (tx, mut rx) = {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let rx = task::spawn(ReleasesOnReady::new(rx, init.add_handle()));
            (tx, rx)
        };
        let mut init = task::spawn(init.initialized());

        assert_pending!(rx.poll());
        assert_pending!(init.poll());
        tx.send("hello").unwrap();
        assert_ready!(rx.poll()).unwrap();
        assert_ready!(init.poll());
    }

    #[tokio::test]
    async fn initializes_on_stream() {
        let mut init = Initialized::default();
        let (tx, mut rx) = {
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            let rx = task::spawn(ReleasesOnReady::new(
                ReceiverStream::new(rx),
                init.add_handle(),
            ));
            (tx, rx)
        };
        let mut init = task::spawn(init.initialized());

        assert_pending!(rx.poll_next());
        assert_pending!(init.poll());
        tx.try_send("hello").unwrap();
        assert_ready!(rx.poll_next());
        assert_ready!(init.poll());
    }
}
