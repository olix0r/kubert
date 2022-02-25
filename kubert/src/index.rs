use async_stream::stream;
use futures_core::Stream;
use futures_util::{future, stream::Scan, StreamExt};
use kube_runtime::watcher;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::{
    sync::{AcquireError, OwnedSemaphorePermit, Semaphore},
    time,
};

/// Issues handles and waits for them to be released
pub struct Initialize {
    semaphore: Arc<Semaphore>,
    issued: u32,
}

/// Signals a component has been initialized
#[must_use]
pub struct Handle(OwnedSemaphorePermit);

/// Processes an `E`-typed event stream of `T`-typed resources.
///
/// The `F`-typed processor is called for each event with exclusive mutable accecss to an `S`-typed
/// store.
///
/// The `H`-typed initialization handle is dropped after the first event is processed to signal to
/// the application that the index has been updated.
///
/// It is assumed that the event stream is infinite. If an error is encountered, the stream is
/// immediately polled again; and if this attempt to read from the stream fails, a backoff is
/// employed before attempting to read from the stream again.
pub async fn index<T, E, H, S, F>(
    events: E,
    initialized: H,
    backoff: time::Duration,
    store: Arc<Mutex<S>>,
    process: F,
) where
    E: Stream<Item = watcher::Result<watcher::Event<T>>>,
    F: Fn(&mut S, watcher::Event<T>),
{
    tokio::pin!(events);

    // A handle to be dropped when the index has been initialized.
    let mut initialized = Some(initialized);

    let mut failed = false;
    while let Some(ev) = events.next().await {
        match ev {
            Ok(ev) => {
                process(&mut *store.lock(), ev);

                // Drop the initialization handle if it's set.
                drop(initialized.take());
                failed = false;
            }

            Err(error) => {
                tracing::info!(%error, "stream failed");
                // If the stream had previously failed, backoff before polling the stream again.
                if failed {
                    // TODO: Use an exponential backoff.
                    tracing::debug!(?backoff);
                    time::sleep(backoff).await;
                }
                failed = true;
            }
        }
    }

    tracing::warn!("k8s event stream terminated");
}

struct BackoffState {
    backoff: time::Duration,
    failed: bool,
}

pub fn handle_errors<S, T, E>(events: S, backoff: time::Duration) -> impl Stream<Item = T>
where
    S: Stream<Item = Result<T, E>>,
    E: std::fmt::Display,
{
    stream! {
        tokio::pin!(events);
        let mut failed = false;
        while let Some(res) = events.next().await {
            match res {
                Ok(ev) => {
                    yield ev;
                    failed = false;
                }
                Err(error) => {
                    tracing::info!(%error, "stream failed");
                    if failed {
                        tracing::debug!(?backoff, "sleeping");
                        time::sleep(backoff).await;
                    }
                    failed = true;
                }
            }
        }
    }
}

impl Default for Initialize {
    fn default() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(0)),
            issued: 0,
        }
    }
}

impl Initialize {
    /// Creates a new [`Handle`]
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
    pub fn drop_on_first<S: Stream>(
        self,
        events: S,
    ) -> Scan<
        S,
        Option<Self>,
        future::Ready<Option<S::Item>>,
        fn(&mut Option<Self>, S::Item) -> future::Ready<Option<S::Item>>,
    > {
        events.scan(Some(self), |handle, ev| {
            drop(handle.take());
            future::ready(Some(ev))
        })
    }
}
