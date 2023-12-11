//! A `prometheus-client` exporter for `tokio-metrics`.

#![deny(rust_2018_idioms, missing_docs, warnings)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(all(feature = "rt", tokio_unstable))]
pub use self::rt::Runtime;

#[cfg(all(feature = "rt", not(tokio_unstable)))]
compile_error!("RUSTFLAGS='--cfg tokio_unstable' must be set to use `tokio-metrics/rt`");

#[cfg(all(feature = "rt", tokio_unstable))]
mod rt {
    use prometheus_client::{
        metrics::{counter::Counter, gauge::Gauge},
        registry::{Registry, Unit},
    };
    use tokio::time;
    use tokio_metrics::{RuntimeIntervals, RuntimeMonitor};

    /// Tokio runtime metrics.
    ///
    /// NOTE that this module requires unstable tokio functionality that must be
    /// enabled via the `tokio_unstable` feature. When it is not enabled, no metrics
    /// will be registered.
    ///
    /// `RUSTFLAGS="--cfg tokio_unstable"` must be set at build-time to use this featur
    #[derive(Debug)]
    pub struct Runtime {
        runtime: tokio::runtime::Handle,
        metrics: Metrics,
    }

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

    impl Runtime {
        /// Registers Tokio runtime metrics with the given registry. Note that
        /// metrics are NOT prefixed.
        pub fn register(reg: &mut Registry, runtime: tokio::runtime::Handle) -> Self {
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

            Self { runtime, metrics }
        }

        /// Drives metrics updates for a runtime according to a fixed interval.
        pub async fn updated(&self, interval: &mut time::Interval) -> ! {
            let mut probes = RuntimeMonitor::new(&self.runtime).intervals();
            loop {
                interval.tick().await;
                self.metrics.probe(&mut probes);
            }
        }
    }

    impl Metrics {
        #[tracing::instrument(skip_all, ret, level = tracing::Level::TRACE)]
        fn probe(&self, probes: &mut RuntimeIntervals) {
            let probe = probes.next().expect("runtime metrics stream must not end");

            // Tokio-metrics tracks all of these values as rates so we have
            // to turn them back into absolute counters:
            self.park.inc_by(probe.total_park_count);
            self.noop.inc_by(probe.total_noop_count);
            self.steal.inc_by(probe.total_steal_count);
            self.steal_operations.inc_by(probe.total_steal_operations);
            self.remote_schedule.inc_by(probe.num_remote_schedules);
            self.local_schedule.inc_by(probe.total_local_schedule_count);
            self.overflow.inc_by(probe.total_overflow_count);
            self.polls.inc_by(probe.total_polls_count);
            self.busy.inc_by(probe.total_busy_duration.as_secs_f64());
            self.io_driver_ready.inc_by(probe.io_driver_ready_count);

            // Instantaneous gauges:
            self.workers.set(probe.workers_count as i64);
            self.injection_queue_depth
                .set(probe.total_local_queue_depth as i64);
            self.local_queue_depth
                .set(probe.total_local_queue_depth as i64);

            // Absolute counters need to be incremented by the delta:
            if let Some(delta) = probe
                .budget_forced_yield_count
                .checked_sub(self.budget_forced_yield.get())
            {
                self.budget_forced_yield.inc_by(delta);
            } else {
                tracing::trace!("budget_forced_yield_count overflow");
            }
        }
    }
}
