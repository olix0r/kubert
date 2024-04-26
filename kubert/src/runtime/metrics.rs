use futures_core::Stream;
use futures_util::StreamExt;
use kube_core::Resource;
use kube_runtime::watcher;
use prometheus_client::{
    encoding::{EncodeLabelSet, EncodeLabelValue},
    metrics::{counter::Counter, family::Family},
    registry::Registry,
};
use std::fmt::Debug;

/// Metrics for tracking resource watch events.
#[derive(Clone, Debug)]
pub(super) struct ResourceWatchMetrics {
    watch_events: Family<EventLabels, Counter>,
    watch_errors: Family<ErrorLabels, Counter>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct EventLabels {
    op: EventOp,
    kind: String,
    group: String,
    version: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ErrorLabels {
    kind: String,
    group: String,
    version: String,
    error: &'static str,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelValue)]
enum EventOp {
    Apply,
    Restart,
    Delete,
}

impl ResourceWatchMetrics {
    /// Creates a new set of metrics and registers them.
    pub(super) fn register(registry: &mut Registry) -> Self {
        let watch_events = Family::default();
        registry.register(
            "events",
            "Count of apply events for a resource watch",
            watch_events.clone(),
        );

        let watch_errors = Family::default();
        registry.register(
            "errors",
            "Count of errors for a resource watch",
            watch_errors.clone(),
        );

        Self {
            watch_events,
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
        T: Resource + Send,
        T::DynamicType: Default,
    {
        let dt = Default::default();
        let kind = T::kind(&dt).into_owned();
        let group = T::group(&dt).into_owned();
        let version = T::version(&dt).into_owned();
        let apply_labels = EventLabels {
            kind,
            group,
            version,
            op: EventOp::Apply,
        };
        let restart_labels = EventLabels {
            op: EventOp::Restart,
            ..apply_labels.clone()
        };
        let delete_labels = EventLabels {
            op: EventOp::Delete,
            ..apply_labels.clone()
        };
        let error_labels = ErrorLabels {
            kind: apply_labels.kind.clone(),
            group: apply_labels.group.clone(),
            version: apply_labels.version.clone(),
            error: "", // replaced later
        };

        watch.map(move |event| {
            if let Some(metrics) = &metrics {
                match event {
                    Ok(watcher::Event::Restarted(_)) => {
                        metrics.watch_events.get_or_create(&restart_labels).inc();
                    }
                    Ok(watcher::Event::Applied(_)) => {
                        metrics.watch_events.get_or_create(&apply_labels).inc();
                    }
                    Ok(watcher::Event::Deleted(_)) => {
                        metrics.watch_events.get_or_create(&delete_labels).inc();
                    }
                    Err(ref e) => {
                        let labels = ErrorLabels {
                            error: match e {
                                watcher::Error::InitialListFailed(_) => "InitialListFailed",
                                watcher::Error::WatchStartFailed(_) => "WatchStartFailed",
                                watcher::Error::WatchError(_) => "WatchError",
                                watcher::Error::WatchFailed(_) => "WatchFailed",
                                watcher::Error::NoResourceVersion => "NoResourceVersion",
                                watcher::Error::TooManyObjects => "TooManyObjects",
                            },
                            ..error_labels.clone()
                        };
                        metrics.watch_errors.get_or_create(&labels).inc();
                    }
                };
            }
            event
        })
    }
}
