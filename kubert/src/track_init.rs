use futures_core::Stream;
use futures_util::{future, stream::Scan, StreamExt};
use kube_runtime::watcher;
use std::sync::Arc;
use tokio::sync::{AcquireError, OwnedSemaphorePermit, Semaphore};

/// Tracks process initialization
///
/// Grants handles to components that need to be initialized and then waits for all handles to be
/// dropped to signal readiness.
pub struct TrackInit {
    semaphore: Arc<Semaphore>,
    issued: u32,
}

/// Signals a component has been initialized
#[must_use]
pub struct Handle(OwnedSemaphorePermit);

impl Default for TrackInit {
    fn default() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(0)),
            issued: 0,
        }
    }
}

impl TrackInit {
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

    /// Waits for all handles to be dropped.
    pub async fn initialized(self) -> Result<(), AcquireError> {
        let _ = self.semaphore.acquire_many(self.issued).await?;
        Ok(())
    }
}

impl Handle {
    /// Wraps the provided [`Stream`] to release the handle when the firstupdate is processed.
    pub fn release_on_next<S: Stream>(self, events: S) -> ReleasesOnNext<S, S::Item> {
        events.scan(Some(self), |handle, ev| {
            drop(handle.take());
            future::ready(Some(ev))
        })
    }
}

/// A `Stream`-wrapper that releases a `Handle` when the first update is processed
pub type ReleasesOnNext<S, T> = Scan<
    S,
    Option<Handle>,
    future::Ready<Option<T>>,
    fn(&mut Option<Handle>, T) -> future::Ready<Option<T>>,
>;
