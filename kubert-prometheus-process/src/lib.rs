//! Process metrics for Prometheus.
//!
//! This crate registers a collector that provides the standard set of [Process
//! metrics][pm].
//!
//! ```
//! let mut prom = prometheus_client::registry::Registry::default();
//! if let Err(error) =
//!     kubert_prometheus_process::register(prom.sub_registry_with_prefix("process"))
//! {
//!     tracing::warn!(%error, "Failed to register process metrics");
//! }
//! ```
//!
//! [pm]: https://prometheus.io/docs/instrumenting/writing_clientlibs/#process-metrics
//
// Based on linkerd2-proxy.
//
// Copyright The Linkerd Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![deny(
    rust_2018_idioms,
    clippy::disallowed_methods,
    unsafe_code,
    missing_docs
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

use prometheus_client::{
    collector::Collector,
    encoding::{DescriptorEncoder, EncodeMetric},
    metrics::{
        counter::ConstCounter,
        gauge::{self, ConstGauge, Gauge},
        MetricType,
    },
    registry::{Registry, Unit},
};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(target_os = "linux")]
mod linux;

/// Registers process metrics with the given registry. Note that the 'process_'
/// prefix is NOT added and should be specified by the caller if desired.
pub fn register(reg: &mut Registry) -> std::io::Result<()> {
    let start_time = Instant::now();
    let start_time_from_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("process start time");

    #[cfg(target_os = "linux")]
    let system = linux::System::load()?;

    reg.register_with_unit(
        "start_time",
        "Time that the process started (in seconds since the UNIX epoch)",
        Unit::Seconds,
        ConstGauge::new(start_time_from_epoch.as_secs_f64()),
    );

    let clock_time_ts = Gauge::<f64, ClockMetric>::default();
    reg.register_with_unit(
        "clock_time",
        "Current system time for this process",
        Unit::Seconds,
        clock_time_ts,
    );

    reg.register_collector(Box::new(ProcessCollector {
        start_time,
        #[cfg(target_os = "linux")]
        system,
    }));

    Ok(())
}

#[derive(Debug)]
struct ProcessCollector {
    start_time: Instant,
    #[cfg(target_os = "linux")]
    system: linux::System,
}

impl Collector for ProcessCollector {
    fn encode(&self, mut encoder: DescriptorEncoder<'_>) -> std::fmt::Result {
        let uptime = ConstCounter::new(
            Instant::now()
                .saturating_duration_since(self.start_time)
                .as_secs_f64(),
        );
        let ue = encoder.encode_descriptor(
            "uptime",
            "Total time since the process started (in seconds)",
            Some(&Unit::Seconds),
            MetricType::Counter,
        )?;
        uptime.encode(ue)?;

        #[cfg(target_os = "linux")]
        self.system.encode(encoder)?;

        Ok(())
    }
}

// Metric that always reports the current system time on a call to [`get`].
#[derive(Copy, Clone, Debug, Default)]
struct ClockMetric;

impl gauge::Atomic<f64> for ClockMetric {
    fn inc(&self) -> f64 {
        self.get()
    }

    fn inc_by(&self, _v: f64) -> f64 {
        self.get()
    }

    fn dec(&self) -> f64 {
        self.get()
    }

    fn dec_by(&self, _v: f64) -> f64 {
        self.get()
    }

    fn set(&self, _v: f64) -> f64 {
        self.get()
    }

    fn get(&self) -> f64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(elapsed) => elapsed.as_secs_f64().floor(),
            Err(e) => {
                tracing::warn!(
                    "System time is before the UNIX epoch; reporting negative timestamp"
                );
                -e.duration().as_secs_f64().floor()
            }
        }
    }
}
