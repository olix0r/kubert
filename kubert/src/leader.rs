//! Uses leases to establish leadership among replicas.

#![allow(missing_docs)]

use k8s_openapi::{api::coordination::v1 as coordv1, apimachinery::pkg::apis::meta::v1::MicroTime};
use tokio::time::Duration;

pub struct Lease {
    api: kube_client::Api<coordv1::Lease>,
    name: String,
    field_manager: String,
    state: tokio::sync::Mutex<State>,
}

#[derive(Clone, Debug)]
pub struct ClaimParams {
    /// The unique identity of the claimant
    pub identity: String,

    /// The duration of the lease
    pub lease_duration: Duration,

    /// The amount of time before the lease expiration that the lease holder
    /// should renew the lease
    pub renew_grace_period: Option<Duration>,
}

#[derive(Clone, Debug)]
pub struct Claim {
    pub holder: String,

    /// The time that the lease expires
    pub expiry: chrono::DateTime<chrono::Utc>,
}

/// Indicates an error interacting with the Lease API
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error was received from the Kubernetes API
    #[error("failed to get lease: {0}")]
    Api(#[from] kube_client::Error),

    /// Lease resource does not have a resourceVersion
    #[error("lease does not have a resource version")]
    MissingResourceVersion,

    /// Lease resource does not have a spec
    #[error("lease does not have a spec")]
    MissingSpec,
}

#[derive(Clone, Debug)]
struct State {
    meta: Meta,
    claim: Option<Claim>,
}

#[derive(Clone, Debug)]
struct Meta {
    version: String,
    transitions: u16,
}

// === impl Claim ===

impl Claim {
    pub fn is_currently_held_by(&self, claimant: &str) -> bool {
        self.holder == claimant && chrono::Utc::now() < self.expiry
    }

    pub async fn sleep_until_expiry(&self) {
        self.sleep_until_before_expiry(Duration::ZERO).await;
    }

    pub async fn sleep_until_before_expiry(&self, grace: Duration) {
        if let Ok(remaining) = (self.expiry - chrono::Utc::now()).to_std() {
            let sleep = remaining.saturating_sub(grace);
            if !sleep.is_zero() {
                tokio::time::sleep(sleep).await;
            }
        }
    }
}

// === impl Lease ===

impl Lease {
    pub async fn init(
        api: kube_client::Api<coordv1::Lease>,
        name: String,
        field_manager: String,
    ) -> Result<Self, Error> {
        let state = Self::sync(api.clone(), &*name).await?;
        Ok(Self {
            api,
            name,
            field_manager,
            state: tokio::sync::Mutex::new(state),
        })
    }

    pub async fn claim(&self, params: &ClaimParams) -> Result<Claim, Error> {
        let mut state = self.state.lock().await;
        loop {
            if let Some(claim) = state.claim.as_ref() {
                let now = chrono::Utc::now();

                if claim.holder == params.identity {
                    let renew_at = claim.expiry
                        - chrono::Duration::from_std(params.renew_grace_period.unwrap_or_default())
                            .unwrap_or_else(|_| chrono::Duration::zero());
                    if now < renew_at {
                        return Ok(claim.clone());
                    }

                    match self.renew(&state.meta, params).await {
                        Ok((claim, meta)) => {
                            *state = State {
                                meta,
                                claim: Some(claim.clone()),
                            };
                            return Ok(claim);
                        }
                        Err(e) if Self::is_conflict(&e) => {
                            *state = Self::sync(self.api.clone(), &*self.name).await?;
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }

                debug_assert!(claim.holder != params.identity);
                if now < claim.expiry {
                    return Ok(claim.clone());
                }
            }

            match self.acquire(&state.meta, params).await {
                Ok((claim, meta)) => {
                    *state = State {
                        meta,
                        claim: Some(claim.clone()),
                    };
                    return Ok(claim);
                }
                Err(e) if Self::is_conflict(&e) => {
                    *state = Self::sync(self.api.clone(), &*self.name).await?;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn acquire(&self, meta: &Meta, params: &ClaimParams) -> Result<(Claim, Meta), Error> {
        let lease_duration = chrono::Duration::from_std(params.lease_duration)
            .unwrap_or_else(|_| chrono::Duration::max_value());
        let now = chrono::Utc::now();
        let lease = self
            .patch(serde_json::json!({
                "metadata": {
                    "resourceVersion": meta.version,
                },
                "spec": {
                    "acquireTime": MicroTime(now),
                    "renewTime": MicroTime(now),
                    "holderIdentity": params.identity,
                    "leaseDurationSeconds": lease_duration.num_seconds(),
                    "leaseTransitions": meta.transitions + 1,
                },
            }))
            .await?;

        let claim = Claim {
            holder: params.identity.clone(),
            expiry: now + lease_duration,
        };
        let meta = Meta {
            version: lease
                .metadata
                .resource_version
                .ok_or(Error::MissingResourceVersion)?,
            transitions: meta.transitions + 1,
        };
        Ok((claim, meta))
    }

    async fn renew(&self, meta: &Meta, params: &ClaimParams) -> Result<(Claim, Meta), Error> {
        let lease_duration = chrono::Duration::from_std(params.lease_duration)
            .unwrap_or_else(|_| chrono::Duration::max_value());
        let now = chrono::Utc::now();
        let lease = self
            .patch(serde_json::json!({
                "metadata": {
                    "resourceVersion": meta.version,
                },
                "spec": {
                    "renewTime": MicroTime(now),
                    "leaseDurationSeconds": lease_duration.num_seconds(),
                },
            }))
            .await?;

        let claim = Claim {
            holder: params.identity.clone(),
            expiry: now + lease_duration,
        };
        let meta = Meta {
            version: lease
                .metadata
                .resource_version
                .ok_or(Error::MissingResourceVersion)?,
            transitions: meta.transitions,
        };
        Ok((claim, meta))
    }

    async fn patch<P>(&self, patch: P) -> Result<coordv1::Lease, kube_client::Error>
    where
        P: serde::Serialize + std::fmt::Debug,
    {
        tracing::debug!(?patch, "acquiring lease");
        let params = kube_client::api::PatchParams {
            field_manager: Some(self.field_manager.clone()),
            ..Default::default()
        };
        self.api
            .patch(&*self.name, &params, &kube_client::api::Patch::Apply(patch))
            .await
    }

    async fn sync(api: kube_client::Api<coordv1::Lease>, name: &str) -> Result<State, Error> {
        let lease = api.get(name).await?;

        let spec = match lease.spec {
            Some(spec) => spec,
            None => return Err(Error::MissingSpec),
        };

        let version = lease
            .metadata
            .resource_version
            .ok_or(Error::MissingResourceVersion)?;
        let transitions = spec.lease_transitions.unwrap_or(0).try_into().unwrap_or(0);
        let meta = Meta {
            version,
            transitions,
        };

        macro_rules! or_unclaimed {
            ($e:expr) => {
                match $e {
                    Some(e) => e,
                    None => {
                        return Ok(State { meta, claim: None });
                    }
                }
            };
        }

        let holder = or_unclaimed!(spec.holder_identity);

        let MicroTime(renew_time) = or_unclaimed!(spec.renew_time);
        let lease_duration =
            chrono::Duration::seconds(or_unclaimed!(spec.lease_duration_seconds).into());
        let expiry = renew_time + lease_duration;
        if expiry <= chrono::Utc::now() {
            return Ok(State { meta, claim: None });
        }

        Ok(State {
            meta,
            claim: Some(Claim { holder, expiry }),
        })
    }

    fn is_conflict(err: &Error) -> bool {
        matches!(
            err,
            Error::Api(kube_client::Error::Api(kube_core::ErrorResponse { code, .. }))
                if hyper::StatusCode::from_u16(*code).ok() == Some(hyper::StatusCode::CONFLICT)
        )
    }
}
