#![deny(warnings, rust_2018_idioms)]
#![forbid(unsafe_code)]

use anyhow::{bail, Result};
use clap::Parser;
use futures::prelude::*;
use k8s_openapi::api::core::v1::Pod;
use kube::{api::ListParams, runtime::watcher::Event, ResourceExt};
use tracing::Instrument;

#[derive(Parser)]
#[clap(version)]
struct Args {
    #[clap(
        long,
        env = "KUBERT_EXAMPLE_LOG",
        default_value = "watch_pods=info,warn"
    )]
    log_level: kubert::LogFilter,

    #[clap(long, default_value = "plain")]
    log_format: kubert::LogFormat,

    #[clap(flatten)]
    client: kubert::ClientArgs,

    #[clap(flatten)]
    admin: kubert::AdminArgs,

    #[clap(long, short = 'l')]
    selector: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let Args {
        log_level,
        log_format,
        client,
        admin,
        selector,
    } = Args::parse();

    // Configure a runtime with:
    // - a Kubernetes client
    // - an admin server with /live and /ready endpoints
    // - a tracing (logging) subscriber
    let mut runtime = kubert::Runtime::builder()
        .with_log(log_level, log_format)
        .with_admin(admin)
        .with_client(client)
        .build()
        .await?;

    // Watch all pods and print changes.
    //
    // This stream completes when shutdown is signaled; and the admin endpoint does not return ready
    // until the first update is received.
    tracing::debug!(?selector);
    let params = selector
        .iter()
        .fold(ListParams::default(), |p, l| p.labels(&l));
    let pods = runtime.watch_all::<Pod>(params);
    tokio::spawn(
        async move {
            tokio::pin!(pods);

            // Keep a list of all known pods so we can identify new and deleted pods on restart.
            // The watch will restart roughly every 5 minutes.
            let mut known = std::collections::HashSet::<(String, String)>::new();
            while let Some(ev) = pods.next().await {
                tracing::trace!(?ev);
                match ev {
                    Event::Restarted(pods) => {
                        tracing::debug!(pods = %pods.len(), "restarted");
                        let mut new = std::collections::HashSet::new();
                        for pod in pods.into_iter() {
                            let namespace = pod.namespace().unwrap();
                            let name = pod.name();
                            let k = (namespace.clone(), name.clone());
                            if !known.contains(&k) {
                                tracing::info!(%namespace, %name, "added")
                            } else {
                                tracing::debug!(%namespace, %name, "already exists")
                            }
                            new.insert(k);
                        }
                        for (namespace, name) in known.into_iter() {
                            if !new.contains(&(namespace.clone(), name.clone())) {
                                tracing::info!(%namespace, %name, "deleted")
                            }
                        }
                        known = new;
                    }

                    Event::Applied(pod) => {
                        let namespace = pod.namespace().unwrap();
                        let name = pod.name();
                        if known.insert((namespace.clone(), name.clone())) {
                            tracing::info!(%namespace, %name, "added");
                        } else {
                            tracing::info!(%namespace, %name, "updated");
                        }
                    }

                    Event::Deleted(pod) => {
                        let namespace = pod.namespace().unwrap();
                        let name = pod.name();
                        tracing::info!(%namespace, %name, "deleted");
                        known.remove(&(namespace, name));
                    }
                }
            }
            tracing::debug!("completed");
        }
        .instrument(tracing::info_span!("pods")),
    );

    // Block the main thread on the shutdown signal. This won't complete until the watch stream
    // stops (after pending Pod updates are logged). If a second signal is received before the watch
    // stream completes, the future fails.
    if runtime.run().await.is_err() {
        bail!("aborted");
    }

    Ok(())
}
