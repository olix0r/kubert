//! Utilities for maintaining a shared index derived from Kubernetes resources.

use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use futures_util::StreamExt;
use kube_core::{Resource, ResourceExt};
use kube_runtime::watcher::Event;
use parking_lot::RwLock;
use std::sync::Arc;

/// Processes updates to `T`-typed cluster-scoped Kubernetes resources.
pub trait IndexClusterResource<T> {
    /// Processes an update to a Kubernetes resource.
    fn apply(&mut self, resource: T);

    /// Observes the removal of a Kubernetes resource.
    fn delete(&mut self, name: String);

    /// Snapshots the names of all `T`-typed resources in the index.
    fn snapshot_keys(&self) -> HashSet<String>;
}

/// Processes updates to `T`-typed namespaced Kubernetes resources.
pub trait IndexNamespacedResource<T> {
    /// Processes an update to a Kubernetes resource.
    fn apply(&mut self, resource: T);

    /// Observes the removal of a Kubernetes resource.
    fn delete(&mut self, namespace: String, name: String);

    /// Snapshots the names of all `T`-typed resources in the index.
    ///
    /// Returns a map of namespaces to sets of names of resources in that namespace.
    fn snapshot_keys(&self) -> HashMap<String, HashSet<String>>;
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

    while let Some(event) = events.next().await {
        tracing::trace!(?event);
        match event {
            Event::Applied(resource) => {
                index.write().apply(resource);
            }

            Event::Deleted(resource) => {
                let namespace = resource
                    .namespace()
                    .expect("resource must have a namespace");
                index.write().delete(namespace, resource.name());
            }

            Event::Restarted(resources) => {
                let mut idx = index.write();

                // Iterate through all the resources in the restarted event and add/update them in the
                // index, keeping track of which resources need to be removed from the index.
                let mut snapshot = idx.snapshot_keys();
                for resource in resources.into_iter() {
                    let namespace = resource
                        .namespace()
                        .expect("resource must have a namespace");
                    let name = resource.name();

                    // If the resource was in the index and is being updated, it doesn't need to be
                    // removed.
                    if let Some(snapshot) = snapshot.get_mut(&namespace) {
                        snapshot.remove(&name);
                    }

                    idx.apply(resource);
                }

                // Remove all resources that were in the index but are no longer in the cluster
                // following a restart.
                for (ns, resources) in snapshot.into_iter() {
                    for name in resources.into_iter() {
                        idx.delete(ns.clone(), name);
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

    while let Some(event) = events.next().await {
        tracing::trace!(?event);
        match event {
            Event::Applied(resource) => {
                index.write().apply(resource);
            }

            Event::Deleted(resource) => {
                index.write().delete(resource.name());
            }

            Event::Restarted(resources) => {
                let mut idx = index.write();

                // Iterate through all the resources in the restarted event and add/update them in the
                // index, keeping track of which resources need to be removed from the index.
                let mut snapshot = idx.snapshot_keys();
                for resource in resources.into_iter() {
                    let name = resource.name();

                    // If the resource was in the index and is being updated, it doesn't need to be
                    // removed.
                    snapshot.remove(&name);

                    idx.apply(resource);
                }

                // Remove all resources that were in the index but are no longer in the cluster
                // following a restart.
                for name in snapshot.into_iter() {
                    idx.delete(name);
                }
            }
        }
    }
}
