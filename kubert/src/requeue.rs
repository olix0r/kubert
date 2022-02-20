use futures::prelude::*;
use kube_core::Resource;
use kube_runtime::reflector::ObjectRef;
use std::{
    collections::{hash_map, HashMap},
    hash::Hash,
};
use tokio::time::Duration;
use tokio_util::time::{delay_queue, DelayQueue};

pub struct Requeue<T>
where
    T: Resource,
    T::DynamicType: PartialEq + Eq + Hash + Clone,
{
    q: DelayQueue<ObjectRef<T>>,
    keys: HashMap<ObjectRef<T>, delay_queue::Key>,
    sleep: tokio::time::Duration,
}

impl<T> Requeue<T>
where
    T: Resource,
    T::DynamicType: PartialEq + Eq + Hash + Clone,
{
    pub fn new(sleep: Duration) -> Self {
        Self {
            q: DelayQueue::new(),
            keys: HashMap::default(),
            sleep,
        }
    }

    pub fn insert(&mut self, key: ObjectRef<T>) {
        match self.keys.entry(key) {
            hash_map::Entry::Occupied(v) => self.q.reset(v.get(), self.sleep),
            hash_map::Entry::Vacant(v) => {
                let key = self.q.insert(v.key().clone(), self.sleep);
                v.insert(key);
            }
        };
    }

    pub async fn next(&mut self) -> Option<ObjectRef<T>> {
        let k = self.q.next().await?.into_inner();
        self.keys.remove(&k);
        Some(k)
    }
}
