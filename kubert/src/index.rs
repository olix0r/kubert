//! Utilities for maintaining a shared index derived from Kubernetes resources.

use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use futures_util::StreamExt;
use kube_core::{Resource, ResourceExt};
use kube_runtime::watcher::Event;
use parking_lot::RwLock;
use std::{collections::hash_map::Entry, sync::Arc};

/// Processes updates to `T`-typed cluster-scoped Kubernetes resources.
pub trait IndexClusterResource<T> {
    /// Processes an update to a Kubernetes resource.
    fn apply(&mut self, resource: T);

    /// Observes the removal of a Kubernetes resource.
    fn delete(&mut self, name: String);
}

/// Processes updates to `T`-typed namespaced Kubernetes resources.
pub trait IndexNamespacedResource<T> {
    /// Processes an update to a Kubernetes resource.
    fn apply(&mut self, resource: T);

    /// Observes the removal of a Kubernetes resource.
    fn delete(&mut self, namespace: String, name: String);
}

/// Updates a `T`-typed index from a watch on a `R`-typed namespaced Kubernetes resource.
pub async fn namespaced<T, R>(
    index: Arc<RwLock<T>>,
    events: impl futures_core::Stream<Item = Event<R>>,
) where
    T: IndexNamespacedResource<R>,
    R: Resource + std::fmt::Debug,
{
    tokio::pin!(events);

    let mut keys = HashMap::new();
    while let Some(event) = events.next().await {
        tracing::trace!(?event);
        match event {
            Event::Applied(resource) => {
                let namespace = resource
                    .namespace()
                    .expect("resource must have a namespace");
                let name = resource.name();
                keys.entry(namespace)
                    .or_insert_with(HashSet::new)
                    .insert(name);
                index.write().apply(resource);
            }

            Event::Deleted(resource) => {
                let namespace = resource
                    .namespace()
                    .expect("resource must have a namespace");
                let name = resource.name();
                if let Entry::Occupied(mut entry) = keys.entry(namespace.clone()) {
                    entry.get_mut().remove(&name);
                    if entry.get().is_empty() {
                        entry.remove();
                    }
                }
                index.write().delete(namespace, name);
            }

            Event::Restarted(resources) => {
                let mut idx = index.write();

                // Iterate through all the resources in the restarted event and add/update them in
                // the index, keeping track of which resources need to be removed from the index.
                let mut prior_keys = keys.clone();
                for resource in resources.into_iter() {
                    let namespace = resource
                        .namespace()
                        .expect("resource must have a namespace");
                    let name = resource.name();
                    if let Some(pk) = prior_keys.get_mut(&namespace) {
                        pk.remove(&name);
                    }
                    keys.entry(namespace)
                        .or_insert_with(HashSet::new)
                        .insert(name);
                    idx.apply(resource);
                }

                // Remove all resources that were in the index but are no longer in the cluster
                // following a restart.
                for (namespace, resources) in prior_keys.into_iter() {
                    for name in resources.into_iter() {
                        if let Entry::Occupied(mut entry) = keys.entry(namespace.clone()) {
                            entry.get_mut().remove(&name);
                            if entry.get().is_empty() {
                                entry.remove();
                            }
                        }
                        idx.delete(namespace.clone(), name);
                    }
                }
            }
        }
    }
}

/// Updates a `T`-typed index from a watch on a `R`-typed cluster-scoped Kubernetes resource.
pub async fn cluster<T, R>(
    index: Arc<RwLock<T>>,
    events: impl futures_core::Stream<Item = Event<R>>,
) where
    T: IndexClusterResource<R>,
    R: Resource + std::fmt::Debug,
{
    tokio::pin!(events);

    let mut keys = HashSet::new();
    while let Some(event) = events.next().await {
        tracing::trace!(?event);
        match event {
            Event::Applied(resource) => {
                keys.insert(resource.name());
                index.write().apply(resource);
            }

            Event::Deleted(resource) => {
                let name = resource.name();
                keys.remove(&name);
                index.write().delete(name);
            }

            Event::Restarted(resources) => {
                let mut idx = index.write();

                // Iterate through all the resources in the restarted event and add/update them in
                // the index, keeping track of which resources need to be removed from the index.
                let mut prior_keys = keys.clone();
                for resource in resources.into_iter() {
                    let name = resource.name();
                    prior_keys.remove(&name);
                    keys.insert(name);
                    idx.apply(resource);
                }

                // Remove all resources that were in the index but are no longer in the cluster
                // following a restart.
                for name in prior_keys.into_iter() {
                    keys.remove(&name);
                    idx.delete(name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::RwLock;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_test::{assert_pending, task};

    #[test]
    fn namespaced_restart() {
        let state = Arc::new(RwLock::new(NsCache(HashMap::new())));
        let (tx, rx) = mpsc::channel(1);
        let mut task = task::spawn(namespaced(state.clone(), ReceiverStream::new(rx)));

        tx.try_send(kube::runtime::watcher::Event::Restarted(
            (0..2)
                .map(|i| k8s_openapi::api::core::v1::Pod {
                    metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                        namespace: Some("default".to_string()),
                        name: Some(format!("pod-{}", i)),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .collect(),
        ))
        .unwrap();
        assert_pending!(task.poll());
        assert_eq!(
            state.read().0,
            Some((
                "default".to_string(),
                vec!["pod-0".to_string(), "pod-1".to_string(),]
                    .into_iter()
                    .collect()
            ))
            .into_iter()
            .collect()
        );

        tx.try_send(kube::runtime::watcher::Event::Restarted(
            (1..3)
                .map(|i| k8s_openapi::api::core::v1::Pod {
                    metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                        namespace: Some("default".to_string()),
                        name: Some(format!("pod-{}", i)),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .collect(),
        ))
        .unwrap();
        assert_pending!(task.poll());
        assert_eq!(
            state.read().0,
            Some((
                "default".to_string(),
                vec!["pod-1".to_string(), "pod-2".to_string(),]
                    .into_iter()
                    .collect()
            ))
            .into_iter()
            .collect()
        );
    }

    #[test]
    fn clustered_restart() {
        let state = Arc::new(RwLock::new(ClusterCache(HashSet::new())));
        let (tx, rx) = mpsc::channel(1);
        let mut task = task::spawn(cluster(state.clone(), ReceiverStream::new(rx)));

        tx.try_send(kube::runtime::watcher::Event::Restarted(
            (0..2)
                .map(|i| k8s_openapi::api::core::v1::Namespace {
                    metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                        namespace: Some("default".to_string()),
                        name: Some(format!("ns-{}", i)),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .collect(),
        ))
        .unwrap();
        assert_pending!(task.poll());
        assert_eq!(
            state.read().0,
            vec!["ns-0".to_string(), "ns-1".to_string()]
                .into_iter()
                .collect()
        );

        tx.try_send(kube::runtime::watcher::Event::Restarted(
            (1..3)
                .map(|i| k8s_openapi::api::core::v1::Namespace {
                    metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                        namespace: Some("default".to_string()),
                        name: Some(format!("ns-{}", i)),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .collect(),
        ))
        .unwrap();
        assert_pending!(task.poll());
        assert_eq!(
            state.read().0,
            vec!["ns-1".to_string(), "ns-2".to_string()]
                .into_iter()
                .collect(),
        );
    }

    struct ClusterCache(HashSet<String>);

    struct NsCache(HashMap<String, HashSet<String>>);

    impl<T: Resource> IndexClusterResource<T> for ClusterCache {
        fn apply(&mut self, resource: T) {
            self.0.insert(resource.name());
        }

        fn delete(&mut self, name: String) {
            self.0.remove(&*name);
        }
    }

    impl<T: Resource> IndexNamespacedResource<T> for NsCache {
        fn apply(&mut self, resource: T) {
            let namespace = resource
                .namespace()
                .expect("resource must have a namespace");
            let name = resource.name();
            self.0
                .entry(namespace)
                .or_insert_with(HashSet::new)
                .insert(name);
        }

        fn delete(&mut self, namespace: String, name: String) {
            if let Entry::Occupied(mut entry) = self.0.entry(namespace) {
                entry.get_mut().remove(&name);
                if entry.get().is_empty() {
                    entry.remove();
                }
            }
        }
    }
}
