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
            tokio::select! {
                k = self.rx.recv(), if !self.rx_closed => match k {
                    Some((k, at)) => match self.pending.entry(k) {
                        hash_map::Entry::Occupied(ent) => {
                            self.q.reset_at(ent.get(), at);
                        }
                        hash_map::Entry::Vacant(slot) => {
                            let key = self.q.insert_at(slot.key().clone(), at);
                            slot.insert(key);
                        }
                    },
                    None => {
                        self.rx_closed = true;
                    },
                },

                exp = self.q.next() => {
                    if let Some(exp) = exp {
                        let k = exp.into_inner();
                        self.pending.remove(&k);
                        return Some(k);
                    }
                }
            }
        }

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
