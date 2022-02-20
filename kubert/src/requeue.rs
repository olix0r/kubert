#![allow(missing_docs)]

use futures::prelude::*;
use kube_core::Resource;
use kube_runtime::reflector::ObjectRef;
use std::{
    collections::{hash_map, HashMap},
    hash::Hash,
};
use tokio::{
    sync::mpsc::{self, error::SendError},
    time::{Duration, Instant},
};
use tokio_util::time::{delay_queue, DelayQueue};

pub struct Sender<T: Resource> {
    tx: mpsc::Sender<(ObjectRef<T>, Instant)>,
}

pub struct Receiver<T>
where
    T: Resource,
    T::DynamicType: PartialEq + Eq + Hash + Clone,
{
    rx: mpsc::Receiver<(ObjectRef<T>, Instant)>,
    rx_closed: bool,
    q: DelayQueue<ObjectRef<T>>,
    pending: HashMap<ObjectRef<T>, delay_queue::Key>,
}

pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>)
where
    T: Resource,
    T::DynamicType: PartialEq + Eq + Hash + Clone,
{
    let (tx, rx) = mpsc::channel(capacity);
    let tx = Sender { tx };
    let rx = Receiver {
        rx,
        rx_closed: false,
        q: DelayQueue::new(),
        pending: HashMap::new(),
    };
    (tx, rx)
}

impl<T> Receiver<T>
where
    T: Resource,
    T::DynamicType: PartialEq + Eq + Hash + Clone,
{
    pub async fn recv(&mut self) -> Option<ObjectRef<T>> {
        while !(self.rx_closed && self.pending.is_empty()) {
            tracing::trace!(rx.closed = self.rx_closed, pending = self.pending.len());
            tokio::select! {
                item = self.rx.recv(), if !self.rx_closed => match item {
                    Some((k, at)) => match self.pending.entry(k) {
                        hash_map::Entry::Occupied(ent) => {
                            tracing::trace!(name = %ent.key().name, "resetting");
                            self.q.reset_at(ent.get(), at);
                        }
                        hash_map::Entry::Vacant(slot) => {
                            tracing::trace!(name = %slot.key().name, "inserting");
                            let key = self.q.insert_at(slot.key().clone(), at);
                            slot.insert(key);
                        }
                    },
                    None => {
                        tracing::trace!("receiver closed");
                        self.rx_closed = true;
                    }
                },

                exp = self.q.next(), if !self.pending.is_empty() => {
                    if let Some(exp) = exp {
                        let key = exp.into_inner();
                        tracing::trace!(name = %key.name, "dequeued");
                        self.pending.remove(&key);
                        return Some(key);
                    }
                }
            }
        }

        tracing::trace!("complete");
        None
    }
}

impl<T: Resource> Sender<T> {
    pub async fn closed(&self) {
        self.tx.closed().await
    }

    pub async fn requeue(
        &self,
        key: ObjectRef<T>,
        wait: Duration,
    ) -> Result<(), SendError<(ObjectRef<T>, Instant)>> {
        self.requeue_at(key, Instant::now() + wait).await
    }

    pub async fn requeue_at(
        &self,
        key: ObjectRef<T>,
        time: Instant,
    ) -> Result<(), SendError<(ObjectRef<T>, Instant)>> {
        self.tx.send((key, time)).await
    }
}

impl<T: Resource> Clone for Sender<T> {
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
    use tokio::time;
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_test::{assert_pending, assert_ready, task};
    use tracing::{info_span, Instrument};

    fn spawn_channel(
        capacity: usize,
    ) -> (Sender<Pod>, task::Spawn<ReceiverStream<ObjectRef<Pod>>>) {
        // Spawn a (mocked) task that reads from the receiver and updates a counter.
        let (rqtx, mut rqrx) = channel::<Pod>(capacity);
        let (tx, rx) = mpsc::channel(capacity);
        let t0 = time::Instant::now();
        tokio::spawn(
            async move {
                loop {
                    tokio::select! {
                        biased;
                        _ = tx.closed() => {
                            tracing::trace!("test sender closed");
                            break;
                        }
                        p = rqrx.recv() => match p {
                            None => {
                                tracing::trace!("requeue receiver closed");
                                break;
                            }
                            Some(pod) => {
                                tracing::debug!(?pod, "dequeued");
                                if tx.send(pod).await.is_err() {
                                    break;
                                }
                                tracing::trace!("pod sent");
                            }
                        }
                    }
                }
                tracing::debug!(uptime = ?time::Instant::now() - t0, "channel complete")
            }
            .instrument(info_span!("requeue worker")),
        );
        (rqtx, task::spawn(ReceiverStream::new(rx)))
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
}
