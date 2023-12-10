#![deny(warnings, rust_2018_idioms)]
#![forbid(unsafe_code)]

use anyhow::Result;
use k8s_openapi::{api::coordination::v1 as coordv1, apimachinery::pkg::apis::meta::v1 as metav1};
use kube::ResourceExt;
use tokio::time;

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

    #[arg(short, long, env = "LOGNAME", default_value = "default")]
    identity: String,

    #[arg(short, long, default_value = "default")]
    namespace: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, clap::Parser)]
enum Command {
    /// Create a Lease
    Create { name: String },

    /// Try to claim a Lease
    Claim {
        #[arg(long, default_value = "30s")]
        duration: Timeout,

        #[arg(long, default_value = "1s")]
        renew_grace_period: Timeout,

        name: String,
    },

    /// Get the status of a Lease
    Get { name: String },

    /// Release a lease if it is currently held by the given identity
    Vacate { name: String },

    Run {
        #[arg(long, default_value = "30s")]
        duration: Timeout,

        #[arg(long, default_value = "1s")]
        renew_grace_period: Timeout,

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
        identity,
        namespace,
        command,
    } = Args::parse();

    let rt = kubert::Runtime::builder()
        .with_log(log_level, log_format)
        .with_client(client);

    match command {
        Command::Create { name } => {
            let rt = rt.build().await?;
            let api = kube::Api::namespaced(rt.client(), &namespace);
            run(rt, async move {
                let lease = api
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
                println!("Created lease: {}", lease.name_unchecked());
                Ok::<_, kubert::lease::Error>(0)
            })
            .await
        }

        Command::Get { name } => {
            let rt = rt.build().await?;
            let api = kube::Api::namespaced(rt.client(), &namespace);
            run(rt, async move {
                let lease = kubert::LeaseManager::init(api, name)
                    .await?
                    .with_field_manager(field_manager);
                match lease.claimed().await {
                    Some(claim) => print_claim(&claim, &identity),
                    None => println!("? Unclaimed"),
                }
                Ok::<_, kubert::lease::Error>(0)
            })
            .await
        }

        Command::Claim {
            duration: Timeout(lease_duration),
            renew_grace_period: Timeout(renew_grace_period),
            name,
        } => {
            let rt = rt.build().await?;
            let api = kube::Api::namespaced(rt.client(), &namespace);
            run(rt, async move {
                let params = kubert::lease::ClaimParams {
                    lease_duration,
                    renew_grace_period,
                };

                let lease = kubert::LeaseManager::init(api, name)
                    .await?
                    .with_field_manager(field_manager);
                let claim = lease.ensure_claimed(&identity, &params).await?;
                print_claim(&claim, &identity);

                Ok::<_, kubert::lease::Error>(!claim.is_current_for(&identity) as i32)
            })
            .await
        }

        Command::Vacate { name } => {
            let rt = rt.build().await?;
            let api = kube::Api::namespaced(rt.client(), &namespace);
            run(rt, async move {
                let released = kubert::LeaseManager::init(api, name)
                    .await?
                    .with_field_manager(field_manager)
                    .vacate(&identity)
                    .await?;
                let code = if released {
                    println!("+ Claim vacated");
                    0
                } else {
                    println!("- Claim not vacated");
                    1
                };
                Ok::<_, kubert::lease::Error>(code)
            })
            .await
        }

        Command::Run {
            duration: Timeout(lease_duration),
            renew_grace_period: Timeout(renew_grace_period),
            name,
        } => {
            let mut prom = prometheus_client::registry::Registry::default();

            let claim_state = prometheus_client::metrics::gauge::Gauge::<i64>::default();
            prom.sub_registry_with_prefix("kubert_lease").register(
                "claimed",
                "Indicates whether this instance is owns the lease",
                claim_state.clone(),
            );

            let state_changes = prometheus_client::metrics::counter::Counter::<u64>::default();
            prom.sub_registry_with_prefix("kubert_lease").register(
                "claim_changes",
                "Counts changes of this process's claim of the lease",
                state_changes.clone(),
            );

            let rt = rt
                .with_admin(admin.into_builder().with_prometheus(prom))
                .build()
                .await?;
            let api = kube::Api::namespaced(rt.client(), &namespace);
            let shutdown = rt.shutdown_handle();
            run(rt, async move {
                let params = kubert::lease::ClaimParams {
                    lease_duration,
                    renew_grace_period,
                };

                let lease = kubert::LeaseManager::init(api, name)
                    .await?
                    .with_field_manager(field_manager);
                let mut claimed = false;
                let (mut claims, task) = lease.spawn(&identity, params).await?;
                loop {
                    claimed = {
                        let claim = claims.borrow_and_update();
                        print_claim(&claim, &identity);
                        match (claimed, claim.is_current_for(&identity)) {
                            (true, true) => true,
                            (false, false) => false,
                            (true, false) => {
                                claim_state.set(0);
                                state_changes.inc();
                                false
                            }
                            (false, true) => {
                                claim_state.set(1);
                                state_changes.inc();
                                true
                            }
                        }
                    };

                    let shutdown = shutdown.clone();
                    tokio::select! {
                        biased;
                        _ = shutdown.signaled() => {
                            return Ok(0);
                        }
                        res = claims.changed() => {
                            if res.is_err() {
                                task.await.expect("task")?;
                                return Ok(0);
                            }
                        }
                    }
                }
            })
            .await
        }
    }
}

async fn run<F>(rt: kubert::Runtime, fut: F) -> Result<()>
where
    F: std::future::Future<Output = Result<i32, kubert::lease::Error>> + Send + 'static,
{
    tokio::select! {
        // Block the main thread on the shutdown signal. This won't complete
        // until the watch stream stops (after pending Pod updates are logged).
        // If a second signal is received before the watch stream completes, the
        // future fails.
        res = rt.run() => res.map_err(Into::into),

        // If the watch stream completes, exit gracefully
        res = fut => {
            let code = res?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        },
    }
}

fn print_claim(claim: &kubert::lease::Claim, identity: &str) {
    let holder = &claim.holder;
    let expiry = claim
        .expiry
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    if !claim.is_current() {
        println!("! Expired for {holder} at {expiry}");
        return;
    }

    println!(
        "{} Claimed by {holder} until {expiry}",
        if claim.holder == identity { "+" } else { "-" }
    );
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
