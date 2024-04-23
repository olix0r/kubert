use futures_core::Stream;
use futures_util::StreamExt;
use kube_core::Resource;
use kube_runtime::watcher;
use prometheus_client::{
    encoding::EncodeLabelSet,
    metrics::{counter::Counter, family::Family},
    registry::Registry,
};
use serde::de::DeserializeOwned;
use std::borrow::Cow;
use std::fmt::Debug;

/// Metrics for tracking resource watch events.
#[derive(Clone, Debug)]
pub(super) struct ResourceWatchMetrics {
    watch_applies: Family<ResourceWatchLabels, Counter>,
    watch_restarts: Family<ResourceWatchLabels, Counter>,
    watch_deletes: Family<ResourceWatchLabels, Counter>,
    watch_errors: Family<ResourceWatchErrorLabels, Counter>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ResourceWatchLabels {
    kind: Cow<'static, str>,
    group: Cow<'static, str>,
    version: Cow<'static, str>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ResourceWatchErrorLabels {
    kind: Cow<'static, str>,
    group: Cow<'static, str>,
    version: Cow<'static, str>,
    error: Cow<'static, str>,
}

impl ResourceWatchMetrics {
    /// Creates a new set of metrics and registers them.
    pub(super) fn register(registry: &mut Registry) -> Self {
        let watch_applies = Family::default();
        registry.register(
            "applies",
            "Count of apply events for a resource watch",
            watch_applies.clone(),
        );

        let watch_restarts = Family::default();
        registry.register(
            "restarts",
            "Count of restart events for a resource watch",
            watch_restarts.clone(),
        );

        let watch_deletes = Family::default();
        registry.register(
            "deletes",
            "Count of delete events for a resource watch",
            watch_deletes.clone(),
        );

        let watch_errors = Family::default();
        registry.register(
            "errors",
            "Count of errors for a resource watch",
            watch_errors.clone(),
        );

        Self {
            watch_applies,
            watch_restarts,
            watch_deletes,
            watch_errors,
        }
    }
}

impl ResourceWatchMetrics {
    pub(crate) fn instrument_watch<T, S: Stream<Item = watcher::Result<watcher::Event<T>>> + Send>(
        metrics: Option<Self>,
        watch: S,
    ) -> impl Stream<Item = watcher::Result<watcher::Event<T>>> + Send
    where
        T: Resource<DynamicType = ()> + DeserializeOwned + Clone + Debug + Send + 'static,
    {
        let kind = T::kind(&());
        let group = T::group(&());
        let version = T::version(&());
        let labels = ResourceWatchLabels {
            kind,
            group,
            version,
        };

        watch.map(move |event| {
            if let Some(metrics) = &metrics {
                match event {
                    Ok(watcher::Event::Applied(_)) => {
                        metrics.watch_applies.get_or_create(&labels).inc();
                    }
                    Ok(watcher::Event::Restarted(_)) => {
                        metrics.watch_restarts.get_or_create(&labels).inc();
                    }
                    Ok(watcher::Event::Deleted(_)) => {
                        metrics.watch_deletes.get_or_create(&labels).inc();
                    }
                    Err(ref e) => {
                        let error = match e {
                            watcher::Error::InitialListFailed(_) => "InitialListFailed",
                            watcher::Error::WatchStartFailed(_) => "WatchStartFailed",
                            watcher::Error::WatchError(_) => "WatchError",
                            watcher::Error::WatchFailed(_) => "WatchFailed",
                            watcher::Error::NoResourceVersion => "NoResourceVersion",
                            watcher::Error::TooManyObjects => "TooManyObjects",
                        };
                        let error_labels = ResourceWatchErrorLabels {
                            kind: labels.kind.clone(),
                            group: labels.group.clone(),
                            version: labels.version.clone(),
                            error: error.into(),
                        };
                        metrics.watch_errors.get_or_create(&error_labels).inc();
                    }
                };
            }
            event
        })
    }
}
