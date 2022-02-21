//! A bounded, delayed, multi-producer, single-consumer queue for deferring work in response to
//! scheduler updates.

use std::{
    collections::{hash_map, HashMap},
    hash::Hash,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    sync::mpsc::{self, error::SendError},
    time::{Duration, Instant},
};
use tokio_util::time::{delay_queue, DelayQueue};

/// Sends delayed values to the associated `Receiver`.
///
/// Instances are created by the [`channel`] function.
pub struct Sender<T> {
    tx: mpsc::Sender<Op<T>>,
}

/// Receives values from associated `Sender`s.
///
/// Instances are created by the [`channel`] function.
pub struct Receiver<T>
where
    T: PartialEq + Eq + Hash,
{
    rx: mpsc::Receiver<Op<T>>,
    rx_closed: bool,
    q: DelayQueue<T>,
    pending: HashMap<T, delay_queue::Key>,
}

/// Creates a bounded, delayed mpsc channel for requeuing controller updates.
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>)
where
    T: PartialEq + Eq + Hash,
{
    let (tx, rx) = mpsc::channel(capacity);
    let rx = Receiver {
        rx,
        rx_closed: false,
        q: DelayQueue::new(),
        pending: HashMap::new(),
    };
    (Sender { tx }, rx)
}

enum Op<T> {
    Requeue(T, Instant),
    Cancel(T),
    Clear,
}

// === impl Receiver ===

impl<T> Receiver<T>
where
    T: Clone + PartialEq + Eq + Hash,
{
    /// Attempts to process requeues, obtaining the next value from the delay queueand registering
    /// current task for wakeup if the value is not yet available, and returning `None` if the
    /// channel is exhausted.
    pub fn poll_requeued(&mut self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        tracing::trace!(rx.closed = self.rx_closed, pending = self.pending.len());

        // We process messages from the sender before looking at the delay queue so that
        // updates have a chance to reset/cancel pending updates.
        if !self.rx_closed {
            loop {
                match self.rx.poll_recv(cx) {
                    Poll::Pending => break,

                    Poll::Ready(None) => {
                        self.rx_closed = true;
                        break;
                    }

                    Poll::Ready(Some(Op::Clear)) => {
                        self.pending.clear();
                        self.q.clear();
                    }

                    Poll::Ready(Some(Op::Cancel(obj))) => {
                        if let Some(key) = self.pending.remove(&obj) {
                            tracing::trace!(?key, "canceling");
                            self.q.remove(&key);
                        }
                    }

                    Poll::Ready(Some(Op::Requeue(k, at))) => match self.pending.entry(k) {
                        hash_map::Entry::Occupied(ent) => {
                            let key = ent.get();
                            tracing::trace!(?key, "resetting");
                            self.q.reset_at(key, at);
                        }
                        hash_map::Entry::Vacant(slot) => {
                            let key = self.q.insert_at(slot.key().clone(), at);
                            tracing::trace!(?key, "inserting");
                            slot.insert(key);
                        }
                    },
                }
            }
        }

        if !self.pending.is_empty() {
            if let Poll::Ready(Some(exp)) = self.q.poll_expired(cx) {
                tracing::trace!(key = ?exp.key(), "dequeued");
                let obj = exp.into_inner();
                self.pending.remove(&obj);
                return Poll::Ready(Some(obj));
            }
        }

        if self.rx_closed && self.pending.is_empty() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

// We never put `T` in a `Pin`...
impl<T: PartialEq + Eq + Hash> Unpin for Receiver<T> {}

impl<T: Clone + PartialEq + Eq + Hash> futures_core::Stream for Receiver<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Receiver::poll_requeued(self.get_mut(), cx)
    }
}

// === impl Receiver ===

impl<T> Sender<T> {
    /// Waits for all receivers to be dropped.
    pub async fn closed(&self) {
        self.tx.closed().await
    }

    /// Cancels all pending work.
    pub async fn clear(&self) -> Result<(), SendError<()>> {
        self.tx
            .send(Op::Clear)
            .await
            .map_err(|SendError(_)| SendError(()))
    }

    /// Schedule the given object to be rescheduled at the given time.
    pub async fn requeue_at(&self, obj: T, time: Instant) -> Result<(), SendError<T>> {
        self.tx
            .send(Op::Requeue(obj, time))
            .await
            .map_err(|SendError(op)| match op {
                Op::Requeue(obj, _) => SendError(obj),
                _ => unreachable!(),
            })
    }

    /// Schedule the given object to be rescheduled after the `defer` time has passed.
    pub async fn requeue(&self, obj: T, defer: Duration) -> Result<(), SendError<T>> {
        self.requeue_at(obj, Instant::now() + defer).await
    }

    /// Cancels pending updates for the given object.
    pub async fn cancel(&self, obj: T) -> Result<(), SendError<T>> {
        self.tx
            .send(Op::Cancel(obj))
            .await
            .map_err(|SendError(op)| match op {
                Op::Cancel(obj) => SendError(obj),
                _ => unreachable!(),
            })
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    pub use super::*;
    use k8s_openapi::api::core::v1::Pod;
    use kube::runtime::reflector::ObjectRef;
    use tokio::time;
    use tokio_test::{assert_pending, assert_ready, task};

    // === utils ===

    // Spawns a task that reads from the receiver and publishes updates on another mpsc. This is all
    // done so that we can use `[task::Spawn`] on a stream type.
    fn spawn_channel(
        capacity: usize,
    ) -> (
        Sender<ObjectRef<Pod>>,
        task::Spawn<Receiver<ObjectRef<Pod>>>,
    ) {
        let (tx, rx) = channel::<ObjectRef<Pod>>(capacity);
        (tx, task::spawn(rx))
    }

    fn init_tracing() -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_test_writer()
                .with_max_level(tracing::Level::TRACE)
                .finish(),
        )
    }

    async fn sleep(d: Duration) {
        let t0 = time::Instant::now();
        time::sleep(d).await;
        tracing::trace!(duration = ?d, ?t0, now = ?time::Instant::now(), "slept")
    }

    // === tests ===

    #[tokio::test(flavor = "current_thread")]
    async fn delays() {
        let _tracing = init_tracing();
        time::pause();
        let (tx, mut rx) = spawn_channel(1);

        let pod_a = ObjectRef::new("pod-a").within("default");
        tx.requeue(pod_a.clone(), Duration::from_secs(10))
            .await
            .expect("must send");
        assert_pending!(rx.poll_next());

        sleep(Duration::from_millis(10001)).await;
        assert_eq!(
            assert_ready!(rx.poll_next()).expect("stream must not end"),
            pod_a
        );
        assert_pending!(rx.poll_next());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn drains_after_sender_dropped() {
        let _tracing = init_tracing();
        time::pause();
        let (tx, mut rx) = spawn_channel(1);

        let pod_a = ObjectRef::new("pod-a").within("default");
        tx.requeue(pod_a.clone(), Duration::from_secs(10))
            .await
            .expect("must send");
        drop(tx);
        assert_pending!(rx.poll_next());

        sleep(Duration::from_secs(11)).await;
        assert_eq!(
            assert_ready!(rx.poll_next()).expect("stream must not end"),
            pod_a
        );
        assert!(assert_ready!(rx.poll_next()).is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resets() {
        let _tracing = init_tracing();
        time::pause();
        let (tx, mut rx) = spawn_channel(1);

        // Requeue a pod
        let pod_a = ObjectRef::new("pod-a").within("default");
        tx.requeue(pod_a.clone(), Duration::from_secs(10))
            .await
            .expect("must send");
        assert_pending!(rx.poll_next());

        // Re-requeue the same pod after 9s.
        sleep(Duration::from_secs(9)).await;
        tx.requeue(pod_a.clone(), Duration::from_secs(10))
            .await
            .expect("must send");

        // Wait until the first requeue would timeout and check that it has not been sent.
        sleep(Duration::from_millis(1001)).await;
        assert_pending!(rx.poll_next());

        // Wait until the second requeue would timeout and check that it has been sent.
        sleep(Duration::from_secs(9)).await;
        assert_eq!(
            assert_ready!(rx.poll_next()).expect("stream must not end"),
            pod_a
        );
        assert_pending!(rx.poll_next());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancels() {
        let _tracing = init_tracing();
        time::pause();
        let (tx, mut rx) = spawn_channel(1);

        // Requeue a pod
        let pod_a = ObjectRef::new("pod-a").within("default");
        tx.requeue(pod_a.clone(), Duration::from_secs(10))
            .await
            .expect("must send");
        assert_pending!(rx.poll_next());

        sleep(Duration::from_millis(10001)).await;
        tx.cancel(pod_a).await.expect("must send cancel");
        assert_pending!(rx.poll_next());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clears() {
        let _tracing = init_tracing();
        time::pause();
        let (tx, mut rx) = spawn_channel(1);

        // Requeue a pod
        let pod_a = ObjectRef::new("pod-a").within("default");
        tx.requeue(pod_a, Duration::from_secs(10))
            .await
            .expect("must send");
        assert_pending!(rx.poll_next());

        sleep(Duration::from_millis(10001)).await;
        tx.clear().await.expect("must send cancel");
        assert_pending!(rx.poll_next());
    }
}
