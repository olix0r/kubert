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

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use libc::{self, pid_t};
    use process::Stat;
    use procfs::{
        process::{self, LimitValue, Process},
        ProcResult,
    };
    use std::time::Duration;
    use std::{fs, io};
    use tracing::{error, warn};

    #[derive(Clone, Debug)]
    pub(super) struct System {
        page_size: u64,
        ms_per_tick: u64,
    }

    impl System {
        pub fn load() -> std::io::Result<Self> {
            let page_size = page_size()?;
            let ms_per_tick = ms_per_tick()?;
            Ok(Self {
                page_size,
                ms_per_tick,
            })
        }
    }

    impl Collector for System {
        fn encode(&self, mut encoder: DescriptorEncoder<'_>) -> std::fmt::Result {
            let stat = match blocking_stat() {
                Ok(stat) => stat,
                Err(error) => {
                    tracing::warn!(%error, "Failed to read process stats");
                    return Ok(());
                }
            };

            let clock_ticks = stat.utime + stat.stime;
            let cpu = ConstCounter::new(
                Duration::from_millis(clock_ticks * self.ms_per_tick).as_secs_f64(),
            );
            let cpue = encoder.encode_descriptor(
                "cpu",
                "Total user and system CPU time spent in seconds",
                Some(&Unit::Seconds),
                MetricType::Counter,
            )?;
            cpu.encode(cpue)?;

            let vm_bytes = ConstGauge::new(stat.vsize as i64);
            let vme = encoder.encode_descriptor(
                "virtual_memory",
                "Virtual memory size in bytes",
                Some(&Unit::Bytes),
                MetricType::Gauge,
            )?;
            vm_bytes.encode(vme)?;

            let rss_bytes = ConstGauge::new((stat.rss * self.page_size) as i64);
            let rsse = encoder.encode_descriptor(
                "resident_memory",
                "Resident memory size in bytes",
                Some(&Unit::Bytes),
                MetricType::Gauge,
            )?;
            rss_bytes.encode(rsse)?;

            match open_fds(stat.pid) {
                Ok(open_fds) => {
                    let fds = ConstGauge::new(open_fds as i64);
                    let fdse = encoder.encode_descriptor(
                        "open_fds",
                        "Number of open file descriptors",
                        None,
                        MetricType::Gauge,
                    )?;
                    fds.encode(fdse)?;
                }
                Err(error) => {
                    tracing::warn!(%error, "Could not determine open fds");
                }
            }

            match max_fds() {
                Ok(max_fds) => {
                    let fds = ConstGauge::new(max_fds as i64);
                    let fdse = encoder.encode_descriptor(
                        "max_fds",
                        "Maximum number of open file descriptors",
                        None,
                        MetricType::Gauge,
                    )?;
                    fds.encode(fdse)?;
                }
                Err(error) => {
                    tracing::warn!(%error, "Could not determine max fds");
                }
            }

            let threads = ConstGauge::new(stat.num_threads);
            let te = encoder.encode_descriptor(
                "threads",
                "Number of OS threads in the process.",
                None,
                MetricType::Gauge,
            )?;
            threads.encode(te)?;

            Ok(())
        }
    }

    fn page_size() -> io::Result<u64> {
        sysconf(libc::_SC_PAGESIZE, "page size")
    }

    fn ms_per_tick() -> io::Result<u64> {
        // On Linux, CLK_TCK is ~always `100`, so pure integer division
        // works. This is probably not suitable if we encounter other
        // values.
        let clock_ticks_per_sec = sysconf(libc::_SC_CLK_TCK, "clock ticks per second")?;
        let ms_per_tick = 1_000 / clock_ticks_per_sec;
        if clock_ticks_per_sec != 100 {
            warn!(
                clock_ticks_per_sec,
                ms_per_tick, "Unexpected value; process_cpu_seconds_total may be inaccurate."
            );
        }
        Ok(ms_per_tick)
    }

    fn blocking_stat() -> ProcResult<Stat> {
        Process::myself()?.stat()
    }

    fn open_fds(pid: pid_t) -> io::Result<u64> {
        let mut open = 0;
        for f in fs::read_dir(format!("/proc/{}/fd", pid))? {
            if !f?.file_type()?.is_dir() {
                open += 1;
            }
        }
        Ok(open)
    }

    fn max_fds() -> ProcResult<u64> {
        let limits = Process::myself()?.limits()?.max_open_files;
        match limits.soft_limit {
            LimitValue::Unlimited => match limits.hard_limit {
                LimitValue::Unlimited => Ok(0),
                LimitValue::Value(hard) => Ok(hard),
            },
            LimitValue::Value(soft) => Ok(soft),
        }
    }

    #[allow(unsafe_code)]
    fn sysconf(num: libc::c_int, name: &'static str) -> Result<u64, io::Error> {
        match unsafe { libc::sysconf(num) } {
            e if e <= 0 => {
                let error = io::Error::last_os_error();
                error!("error getting {}: {:?}", name, error);
                Err(error)
            }
            val => Ok(val as u64),
        }
    }
}
