//! A `tokio-metrics` exporter for `prometheus-client`.
//!
//! NOTE that this crate requires unstable tokio features that must be enabled
//! via the `tokio_unstable` feature to be enabled via `RUSTFLAGS="--cfg
//! tokio_unstable"`. When it is not enabled, no metrics will be registered.

#![deny(rust_2018_idioms, missing_docs, warnings)]
#![forbid(unsafe_code)]
#![cfg(tokio_unstable)]
#![cfg_attr(docsrs, feature(doc_cfg))]

use prometheus_client::{
    metrics::{counter::Counter, gauge::Gauge},
    registry::{Registry, Unit},
};
use tokio::time;
use tokio_metrics::RuntimeMonitor;
use tracing::Instrument;

#[derive(Debug, Default)]
struct Metrics {
    workers: Gauge,
    park: Counter,
    noop: Counter,
    steal: Counter,
    steal_operations: Counter,
    remote_schedule: Counter,
    local_schedule: Counter,
    overflow: Counter,
    polls: Counter,
    busy: Counter<f64>,
    injection_queue_depth: Gauge,
    local_queue_depth: Gauge,
    budget_forced_yield: Counter,
    io_driver_ready: Counter,
    // TODO poll_count_histogram requires configuration
}

/// Registers Tokio runtime metrics with the given registry. Note that the 'tokio_'
/// prefix is NOT added and should be specified by the caller if desired.
///
/// Returns a handle to the spawned task that updates the metrics every `interval`.
pub fn register_spawned_interval(
    reg: &mut Registry,
    rt: &tokio::runtime::Handle,
    interval: time::Duration,
) -> tokio::task::JoinHandle<()> {
    let reg = reg.sub_registry_with_prefix("rt");

    let metrics = Metrics::default();
    reg.register(
        "workers",
        "The number of worker threads used by the runtime",
        metrics.workers.clone(),
    );
    reg.register(
        "park",
        "Total number of times worker threads parked",
        metrics.park.clone(),
    );
    reg.register(
        "noop",
        "Number of times workers unparked but found no new work",
        metrics.noop.clone(),
    );
    reg.register(
        "steal",
        "Number of tasks stolen by workers from others",
        metrics.steal.clone(),
    );
    reg.register(
        "steal_operations",
        "Number of times workers stole tasks from other",
        metrics.steal_operations.clone(),
    );
    reg.register(
        "remote_schedule",
        "Total number of remote schedule operations",
        metrics.remote_schedule.clone(),
    );
    reg.register(
        "local_schedule",
        "Total number of local schedule operations",
        metrics.local_schedule.clone(),
    );
    reg.register(
        "overflow",
        "Total number of overflow operations",
        metrics.overflow.clone(),
    );
    reg.register(
        "polls",
        "The number of tasks that have been polled across all worker threads",
        metrics.polls.clone(),
    );
    reg.register_with_unit(
        "busy",
        "Total duration of time when worker threads were busy processing tasks",
        Unit::Seconds,
        metrics.busy.clone(),
    );
    reg.register(
        "injection_queue_depth",
        "The number of tasks currently scheduled in the runtime's injection queue",
        metrics.injection_queue_depth.clone(),
    );
    reg.register(
        "local_queue_depth",
        "The total number of tasks currently scheduled in workers' local queues",
        metrics.local_queue_depth.clone(),
    );

    reg.register(
        "budget_forced_yield",
        "Number of times a worker thread was forced to yield due to budget exhaustion",
        metrics.budget_forced_yield.clone(),
    );
    reg.register(
        "io_driver_ready",
        "Number of times the IO driver was woken up",
        metrics.io_driver_ready.clone(),
    );

    let monitor = RuntimeMonitor::new(rt);
    tokio::spawn(
        async move {
            let mut interval = time::interval(interval);
            let mut runtime = monitor.intervals();
            loop {
                interval.tick().await;

                tracing::trace!("Updating Tokio runtime metrics");
                let m = runtime.next().expect("runtime metrics stream must not end");

                // Tokio-metrics tracks all of these values as rates so we have
                // to turn them back into absolute counters:
                metrics.workers.set(m.workers_count as i64);
                metrics.park.inc_by(m.total_park_count);
                metrics.noop.inc_by(m.total_noop_count);
                metrics.steal.inc_by(m.total_steal_count);
                metrics.steal_operations.inc_by(m.total_steal_operations);
                metrics.remote_schedule.inc_by(m.num_remote_schedules);
                metrics.local_schedule.inc_by(m.total_local_schedule_count);
                metrics.overflow.inc_by(m.total_overflow_count);
                metrics.polls.inc_by(m.total_polls_count);
                metrics.busy.inc_by(m.total_busy_duration.as_secs_f64());
                metrics.io_driver_ready.inc_by(m.io_driver_ready_count);

                // Instantaneous gauges:
                metrics
                    .injection_queue_depth
                    .set(m.total_local_queue_depth as i64);
                metrics
                    .local_queue_depth
                    .set(m.total_local_queue_depth as i64);

                // Absolute counters need to be incremented by the delta:
                if let Some(delta) = m
                    .budget_forced_yield_count
                    .checked_sub(metrics.budget_forced_yield.get())
                {
                    metrics.budget_forced_yield.inc_by(delta);
                } else {
                    tracing::warn!("budget_forced_yield_count overflow",);
                }

                tracing::trace!("Updated Tokio runtime metrics");
            }
        }
        .instrument(tracing::info_span!("kubert-prometheus-tokio")),
    )
}
