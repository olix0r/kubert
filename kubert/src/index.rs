//! Utilities for maintaining a shared index derived from Kubernetes resources.

use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use futures_util::StreamExt;
use kube_core::{Resource, ResourceExt};
use kube_runtime::watcher::Event;
use parking_lot::RwLock;
use std::{collections::hash_map::Entry, sync::Arc};

/// A set of the names of cluster-level resources that have been removed.
pub type ClusterRemoved = HashSet<String>;

/// A set ofnames of resources that have been removed grouped by namespace.
pub type NamespacedRemoved = HashMap<String, HashSet<String>>;

/// Processes updates to `T`-typed cluster-scoped Kubernetes resources.
pub trait IndexClusterResource<T> {
    /// Processes an update to a Kubernetes resource.
    fn apply(&mut self, resource: T);

    /// Observes the removal of a Kubernetes resource.
    fn delete(&mut self, name: String);

    /// Resets the index with the given set of live resources and the set of keys that were removed.
    ///
    /// The default implementation calls `apply` and `delete`.
    fn reset(&mut self, resources: Vec<T>, removed: ClusterRemoved) {
        for resource in resources.into_iter() {
            self.apply(resource);
        }

        for name in removed.into_iter() {
            self.delete(name);
        }
    }
}

/// Processes updates to `T`-typed namespaced Kubernetes resources.
pub trait IndexNamespacedResource<T> {
    /// Processes an update to a Kubernetes resource.
    fn apply(&mut self, resource: T);

    /// Observes the removal of a Kubernetes resource.
    fn delete(&mut self, namespace: String, name: String);

    /// Resets an index with a set of live resources and a namespaced map of removed
    ///
    /// The default implementation calls `apply` and `delete`.
    fn reset(&mut self, resources: Vec<T>, removed: NamespacedRemoved) {
        for resource in resources.into_iter() {
            self.apply(resource);
        }

        for (ns, names) in removed.into_iter() {
            for name in names.into_iter() {
                self.delete(ns.clone(), name);
            }
        }
    }
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
                let name = resource.name_unchecked();

                keys.entry(namespace)
                    .or_insert_with(HashSet::new)
                    .insert(name);

                index.write().apply(resource);
            }

            Event::Deleted(resource) => {
                let namespace = resource
                    .namespace()
                    .expect("resource must have a namespace");
                let name = resource.name_unchecked();

                if let Entry::Occupied(mut entry) = keys.entry(namespace.clone()) {
                    entry.get_mut().remove(&name);
                    if entry.get().is_empty() {
                        entry.remove();
                    }
                }

                index.write().delete(namespace, name);
            }

            Event::Restarted(resources) => {
                // Iterate through all the resources in the restarted event and add/update them in
                // the index, keeping track of which resources need to be removed from the index.
                let mut removed = keys.clone();
                for resource in resources.iter() {
                    let namespace = resource
                        .namespace()
                        .expect("resource must have a namespace");
                    let name = resource.name_unchecked();

                    if let Some(names) = removed.get_mut(&namespace) {
                        names.remove(&name);
                    }

                    keys.entry(namespace).or_default().insert(name);
                }

                // Remove all resources that were in the index but are no longer in the cluster
                // following a restart.
                for (namespace, names) in removed.iter() {
                    if let Entry::Occupied(mut entry) = keys.entry(namespace.clone()) {
                        for name in names.iter() {
                            entry.get_mut().remove(name);
                        }
                        if entry.get().is_empty() {
                            entry.remove();
                        }
                    }
                }

                index.write().reset(resources, removed);
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
                keys.insert(resource.name_unchecked());
                index.write().apply(resource);
            }

            Event::Deleted(resource) => {
                let name = resource.name_unchecked();
                keys.remove(&name);
                index.write().delete(name);
            }

            Event::Restarted(resources) => {
                // Iterate through all the resources in the restarted event and add/update them in
                // the index, keeping track of which resources need to be removed from the index.
                let mut removed = keys.clone();
                for resource in resources.iter() {
                    let name = resource.name_unchecked();
                    removed.remove(&name);
                    keys.insert(name);
                }

                // Remove all resources that were in the index but are no longer in the cluster
                // following a restart.
                for name in removed.iter() {
                    keys.remove(name);
                }

                index.write().reset(resources, removed);
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
            self.0.insert(resource.name_unchecked());
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
            let name = resource.name_unchecked();
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
