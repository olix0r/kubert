#![deny(warnings, rust_2018_idioms)]
#![forbid(unsafe_code)]

use anyhow::{bail, Result};
use clap::Parser;
use futures::prelude::*;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    runtime::watcher::{self, Event},
    ResourceExt,
};
use prometheus_client::metrics::{counter::Counter, family::Family, gauge::Gauge};
use tokio::time;
use tracing::Instrument;

#[derive(Clone, Parser)]
#[clap(version)]
struct Args {
    /// The tracing filter used for logs
    #[clap(
        long,
        env = "KUBERT_EXAMPLE_LOG",
        default_value = "watch_pods=info,warn"
    )]
    log_level: kubert::LogFilter,

    /// The logging format
    #[clap(long, default_value = "plain")]
    log_format: kubert::LogFormat,

    #[clap(flatten)]
    client: kubert::ClientArgs,

    #[clap(flatten)]
    admin: kubert::AdminArgs,

    /// Exit after the first update is received
    #[clap(long)]
    exit: bool,

    /// The amount of time to wait for the first update
    #[clap(long, default_value = "10s")]
    timeout: Timeout,

    /// An optional pod selector
    #[clap(long, short = 'l')]
    selector: Option<String>,
}

#[derive(Clone, Debug)]
struct Metrics {
    events_restart: Counter<u64>,
    events_apply: Counter<u64>,
    events_delete: Counter<u64>,
    current_pods: Gauge<i64>,
    total_pods: Counter<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let Args {
        log_level,
        log_format,
        client,
        admin,
        exit,
        timeout: Timeout(timeout),
        selector,
    } = Args::parse();

    let mut prom = prometheus_client::registry::Registry::default();

    // Register application metrics before configuring the admin server.
    let metrics = Metrics::register(prom.sub_registry_with_prefix("kubert_watch_pods"));

    // Configure a runtime with:
    // - a Kubernetes client
    // - an admin server with /live, /ready, and /metrics endpoints
    // - a tracing (logging) subscriber
    let rt = kubert::Runtime::builder()
        .with_log(log_level, log_format)
        .with_admin(admin.into_builder().with_prometheus(prom))
        .with_client(client);

    let deadline = time::Instant::now() + timeout;
    let mut runtime = match time::timeout_at(deadline, rt.build()).await {
        Ok(res) => res?,
        Err(_) => bail!("Timed out waiting for Kubernetes client to initialize"),
    };

    // Watch all pods and print changes.
    //
    // This stream completes when shutdown is signaled; and the admin endpoint does not return ready
    // until the first update is received.
    tracing::debug!(?selector);
    let watcher_config = selector
        .iter()
        .fold(watcher::Config::default(), |p, l| p.labels(l));
    let pods = runtime.watch_all::<Pod>(watcher_config);
    let mut deadline = Some(deadline);
    let task = tokio::spawn(
        async move {
            tokio::pin!(pods);

            // Keep a list of all known pods so we can identify new and deleted pods on restart.
            // The watch will restart roughly every 5 minutes.
            let mut known = std::collections::HashSet::<(String, String)>::new();
            while let Some(ev) = init_timeout(deadline.take(), pods.next()).await? {
                tracing::trace!(?ev);
                match ev {
                    Event::Restarted(pods) => {
                        metrics.events_restart.inc();
                        tracing::debug!(pods = %pods.len(), "Restarted");

                        let mut prior = std::mem::take(&mut known);
                        for pod in pods.into_iter() {
                            let namespace = pod.namespace().unwrap();
                            let name = pod.name_unchecked();
                            let k = (namespace.clone(), name.clone());
                            if prior.remove(&k) {
                                tracing::debug!(%namespace, %name, "Already exists")
                            } else {
                                metrics.current_pods.inc();
                                metrics.total_pods.inc();
                                tracing::info!(%namespace, %name, "Added")
                            }
                            known.insert(k);
                        }
                        for (namespace, name) in prior.into_iter() {
                            metrics.current_pods.dec();
                            tracing::info!(%namespace, %name, "Deleted")
                        }
                    }

                    Event::Applied(pod) => {
                        metrics.events_apply.inc();
                        let namespace = pod.namespace().unwrap();
                        let name = pod.name_unchecked();
                        if known.insert((namespace.clone(), name.clone())) {
                            metrics.current_pods.inc();
                            metrics.total_pods.inc();
                            tracing::info!(%namespace, %name, "Added");
                        } else {
                            tracing::info!(%namespace, %name, "Updated");
                        }
                    }

                    Event::Deleted(pod) => {
                        metrics.events_delete.inc();
                        let namespace = pod.namespace().unwrap();
                        let name = pod.name_unchecked();
                        tracing::info!(%namespace, %name, "Deleted");
                        if known.remove(&(namespace, name)) {
                            metrics.current_pods.dec();
                        }
                    }
                }

                if exit {
                    return Ok::<_, anyhow::Error>(());
                }
            }
            tracing::debug!("completed");
            Ok(())
        }
        .instrument(tracing::info_span!("pods")),
    );

    tokio::select! {
        // Block the main thread on the shutdown signal. This won't complete until the watch stream
        // stops (after pending Pod updates are logged). If a second signal is received before the watch
        // stream completes, the future fails.
        res = runtime.run() => {
            if res.is_err() {
                bail!("aborted");
            }
        }

        // If the watch stream completes, exit gracefully
        res = task => match res {
            Err(error) => bail!("spawned task failed: {}", error),
            Ok(Err(_)) => bail!("Timed out waiting for the first update"),
            Ok(Ok(())) => {
                tracing::debug!("watch completed");
            }
        },
    }

    Ok(())
}

#[derive(Copy, Clone, Debug)]
struct Timeout(time::Duration);

#[derive(Copy, Clone, Debug, thiserror::Error)]
#[error("invalid duration")]
struct InvalidTimeout;

impl std::str::FromStr for Timeout {
    type Err = InvalidTimeout;

    fn from_str(s: &str) -> Result<Self, InvalidTimeout> {
        let re = regex::Regex::new(r"^\s*(\d+)(ms|s|m)?\s*$").expect("duration regex");
        let cap = re.captures(s).ok_or(InvalidTimeout)?;
        let magnitude = cap[1].parse().map_err(|_| InvalidTimeout)?;
        let t = match cap.get(2).map(|m| m.as_str()) {
            None if magnitude == 0 => time::Duration::from_millis(0),
            Some("ms") => time::Duration::from_millis(magnitude),
            Some("s") => time::Duration::from_secs(magnitude),
            Some("m") => time::Duration::from_secs(magnitude * 60),
            _ => return Err(InvalidTimeout),
        };
        Ok(Self(t))
    }
}

async fn init_timeout<F: Future>(deadline: Option<time::Instant>, future: F) -> Result<F::Output> {
    if let Some(deadline) = deadline {
        return time::timeout_at(deadline, future).await.map_err(Into::into);
    }

    Ok(future.await)
}

impl Metrics {
    fn register(prom: &mut prometheus_client::registry::Registry) -> Self {
        let events = Family::<_, Counter<u64>>::default();
        let events_restart = events.get_or_create(&[("op", "restart")]).clone();
        let events_apply = events.get_or_create(&[("op", "apply")]).clone();
        let events_delete = events.get_or_create(&[("op", "delete")]).clone();
        prom.register("events", "Number of events observed", events.clone());

        let current_pods = Gauge::<i64>::default();
        prom.register(
            "current_pods",
            "Number of Pods being observed",
            current_pods.clone(),
        );

        let total_pods = Counter::<u64>::default();
        prom.register(
            "pods",
            "Total number of unique pods observed",
            total_pods.clone(),
        );

        Self {
            events_restart,
            events_apply,
            events_delete,
            current_pods,
            total_pods,
        }
    }
}
