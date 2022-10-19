#![allow(warnings, rust_2018_idioms)]
#![forbid(unsafe_code)]

use anyhow::{bail, Result};
use futures::prelude::*;
use k8s_openapi::{api::coordination::v1 as coordv1, apimachinery::pkg::apis::meta::v1 as metav1};
use kube::{api::ListParams, runtime::watcher::Event, ResourceExt};
use tokio::time;
use tracing::Instrument;

#[derive(Clone, clap::Parser)]
#[clap(version)]
struct Args {
    /// The tracing filter used for logs
    #[arg(long, env = "KUBERT_EXAMPLE_LOG", default_value = "lease=info,warn")]
    log_level: kubert::LogFilter,

    /// The logging format
    #[arg(long, default_value = "plain")]
    log_format: kubert::LogFormat,

    #[clap(flatten)]
    client: kubert::ClientArgs,

    #[clap(flatten)]
    admin: kubert::AdminArgs,

    #[arg(long, default_value = "kubert-examples")]
    field_manager: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, clap::Parser)]
enum Command {
    /// Create a Lease
    Create {
        #[arg(short, long, default_value = "default")]
        namespace: String,

        name: String,
    },

    /// Try to claim a Lease
    Claim {
        #[arg(long, default_value = "30s")]
        duration: Timeout,

        #[arg(long)]
        renew_grace_period: Option<Timeout>,

        #[arg(short, long, env = "LOGNAME", default_value = "default")]
        identity: String,

        #[arg(short, long, default_value = "default")]
        namespace: String,

        name: String,
    },

    /// Get the status of a Lease
    Get {
        #[arg(short, long, default_value = "default")]
        namespace: String,

        name: String,
    },

    /// Release a lease if it is currently held by the given identity
    Abdicate {
        #[arg(short, long, env = "LOGNAME", default_value = "default")]
        identity: String,

        #[arg(short, long, default_value = "default")]
        namespace: String,

        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    use clap::Parser;

    let Args {
        log_level,
        log_format,
        client,
        admin,
        field_manager,
        command,
    } = Args::parse();

    // Configure a runtime with:
    // - a Kubernetes client
    // - an admin server with /live and /ready endpoints
    // - a tracing (logging) subscriber
    let rt = kubert::Runtime::builder()
        .with_log(log_level, log_format)
        .with_admin(admin)
        .with_client(client)
        .build()
        .await?;

    let client = rt.client();
    let task = match command {
        Command::Create { namespace, name } => tokio::spawn(async move {
            let lease = kube::Api::namespaced(client, &namespace)
                .create(
                    &Default::default(),
                    &coordv1::Lease {
                        metadata: metav1::ObjectMeta {
                            name: Some(name),
                            namespace: Some(namespace),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
                .await?;
            println!("Created lease: {}", lease.name());
            Ok::<_, kubert::lease::Error>(())
        }),

        Command::Get { namespace, name } => tokio::spawn(async move {
            let api = kube::Api::namespaced(client, &namespace);
            let lease = kubert::Lease::init(api, name, field_manager).await?;
            match lease.sync().await? {
                Some(kubert::lease::Claim { holder, expiry }) => {
                    println!("Claimed by {holder} until {expiry}");
                }
                None => println!("Unclaimed"),
            }
            Ok::<_, kubert::lease::Error>(())
        }),

        Command::Claim {
            duration: Timeout(duration),
            renew_grace_period,
            identity,
            namespace,
            name,
        } => tokio::spawn(async move {
            let params = kubert::lease::ClaimParams {
                identity,
                lease_duration: duration,
                renew_grace_period: renew_grace_period.map(|Timeout(d)| d),
            };

            let api = kube::Api::namespaced(client, &namespace);
            let lease = kubert::Lease::init(api, name, field_manager).await?;
            let kubert::lease::Claim { holder, expiry } = lease.claim(&params).await?;
            println!("Claimed by {holder} until {expiry}");

            Ok::<_, kubert::lease::Error>(())
        }),

        Command::Abdicate {
            identity,
            namespace,
            name,
        } => tokio::spawn(async move {
            let api = kube::Api::namespaced(client, &namespace);
            let released = kubert::Lease::init(api, name, field_manager)
                .await?
                .release(&*identity)
                .await?;
            if released {
                println!("Abdicated");
            } else {
                println!("Not abdicated");
            }

            Ok::<_, kubert::lease::Error>(())
        }),
    };

    tokio::select! {
        // Block the main thread on the shutdown signal. This won't complete until the watch stream
        // stops (after pending Pod updates are logged). If a second signal is received before the watch
        // stream completes, the future fails.
        res = rt.run() => {
            if res.is_err() {
                bail!("aborted");
            }
        }

        // If the watch stream completes, exit gracefully
        res = task => match res {
            Ok(Ok(())) => {}
            Err(error) => bail!(error),
            Ok(Err(error)) => bail!(error),
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
