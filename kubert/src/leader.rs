//! Uses leases to establish leadership among replicas.

#![allow(warnings, missing_docs)]

use k8s_openapi::{api::coordination::v1 as coordv1, apimachinery::pkg::apis::meta::v1::MicroTime};
use tokio::time::{Duration, Instant};

pub struct Lease {
    field_manager: String,
    identity: String,
    client: kube_client::Client,
    namespace: String,
    name: String,
}

#[derive(Clone, Debug)]
struct Meta {
    version: String,
    transitions: u16,
}

enum State {
    Held {
        meta: Meta,
        until: Instant,
    },
    HeldByAnother {
        meta: Meta,
        holder_id: String,
        until: Instant,
    },
    Released(Meta),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to get lease: {0}")]
    Api(#[from] kube_client::Error),

    #[error("lease does not have a resource version")]
    MissingResourceVersion,

    #[error("lease does not have a spec")]
    MissingSpec,
}

impl Lease {
    pub fn new(
        field_manager: String,
        identity: String,
        client: kube_client::Client,
        namespace: String,
        name: String,
    ) -> Self {
        Self {
            field_manager,
            identity,
            client,
            namespace,
            name,
        }
    }

    pub fn spawn(
        self,
        lease_duration: Duration,
    ) -> (
        tokio::task::JoinHandle<Result<(), Error>>,
        tokio::sync::watch::Receiver<bool>,
    ) {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            let mut state = self.get_state().await?;
            loop {
                let res = match state {
                    State::Held { meta, until } => {
                        if tx.send(true).is_err() {
                            return Ok(());
                        }
                        tokio::time::sleep_until(until - Duration::from_secs(3)).await;
                        self.try_renew(lease_duration, meta).await
                    }
                    State::HeldByAnother { meta, until, .. } => {
                        if tx.send(false).is_err() {
                            return Ok(());
                        }
                        tokio::time::sleep_until(until).await;
                        self.try_acquire(lease_duration, meta).await
                    }
                    State::Released(meta) => {
                        if tx.send(false).is_err() {
                            return Ok(());
                        }
                        self.try_acquire(lease_duration, meta).await
                    }
                };
                state = match res {
                    Ok(s) => s,
                    Err(e) if e.is_conflict() => {
                        tracing::warn!(error = ?e, "conflict while trying to acquire lease");
                        self.get_state().await?
                    }
                    Err(e) => return Err(e),
                };
            }
        });
        (task, rx)
    }

    async fn get_state(&self) -> Result<State, Error> {
        let api =
            kube_client::Api::<coordv1::Lease>::namespaced(self.client.clone(), &*self.namespace);
        let lease = api.get(&*self.name).await?;
        let version = lease
            .metadata
            .resource_version
            .ok_or(Error::MissingResourceVersion)?;

        let spec = match lease.spec {
            Some(spec) => spec,
            None => return Err(Error::MissingSpec),
        };

        let transitions = spec.lease_transitions.unwrap_or(0).try_into().unwrap_or(0);

        let meta = Meta {
            version,
            transitions,
        };

        macro_rules! or_released {
            ($e:expr) => {
                match $e {
                    Some(e) => e,
                    None => {
                        return Ok(State::Released(meta));
                    }
                }
            };
        }

        let holder_id = or_released!(spec.holder_identity);

        let MicroTime(renew_time) = or_released!(spec.renew_time);
        let lease_duration =
            chrono::Duration::seconds(or_released!(spec.lease_duration_seconds).into());
        let released_at = renew_time + lease_duration;
        let remaining = or_released!((released_at - chrono::Utc::now()).to_std().ok());
        let until = Instant::now() + remaining;
        if remaining.is_zero() {
            return Ok(State::Released(meta));
        }

        if holder_id == self.identity {
            return Ok(State::Held { meta, until });
        }

        Ok(State::HeldByAnother {
            meta,
            holder_id,
            until,
        })
    }

    async fn try_acquire(&self, duration: Duration, meta: Meta) -> Result<State, Error> {
        let until = Instant::now() + duration;
        let now = chrono::Utc::now();
        let patch = serde_json::json!({
            "metadata": {
                "resourceVersion": meta.version,
            },
            "spec": {
                "acquireTime": MicroTime(now),
                "renewTime": MicroTime(now),
                "holderIdentity": self.identity,
                "leaseDurationSeconds": duration.as_secs(),
                "leaseTransitions": meta.transitions + 1,
            },
        });
        tracing::debug!(?patch, "acquiring lease");

        let params = kube_client::api::PatchParams {
            field_manager: Some(self.field_manager.clone()),
            ..Default::default()
        };
        let lease =
            kube_client::Api::<coordv1::Lease>::namespaced(self.client.clone(), &*self.namespace)
                .patch(&*self.name, &params, &kube_client::api::Patch::Apply(patch))
                .await?;
        Ok(State::Held {
            until,
            meta: Meta {
                version: lease
                    .metadata
                    .resource_version
                    .ok_or(Error::MissingResourceVersion)?,
                transitions: meta.transitions + 1,
            },
        })
    }

    async fn try_renew(&self, duration: Duration, meta: Meta) -> Result<State, Error> {
        let until = Instant::now() + duration;
        let now = chrono::Utc::now();
        let patch = serde_json::json!({
            "metadata": {
                "resourceVersion": meta.version,
            },
            "spec": {
                "renewTime": MicroTime(now),
                "leaseDurationSeconds": duration.as_secs(),
            },
        });
        tracing::debug!(?patch, "acquiring lease");

        let params = kube_client::api::PatchParams {
            field_manager: Some(self.field_manager.clone()),
            ..Default::default()
        };
        let lease =
            kube_client::Api::<coordv1::Lease>::namespaced(self.client.clone(), &*self.namespace)
                .patch(&*self.name, &params, &kube_client::api::Patch::Apply(patch))
                .await?;
        Ok(State::Held {
            until,
            meta: Meta {
                version: lease
                    .metadata
                    .resource_version
                    .ok_or(Error::MissingResourceVersion)?,
                ..meta
            },
        })
    }
}

impl Error {
    fn is_conflict(&self) -> bool {
        matches!(
            self,
            Self::Api(kube_client::Error::Api(kube_core::ErrorResponse { code, .. }))
                if hyper::StatusCode::from_u16(*code).ok() == Some(hyper::StatusCode::CONFLICT)
        )
    }
}
