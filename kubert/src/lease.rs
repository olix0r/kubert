//! A distributed, advisory lock implementation for Kubernetes
//!
//! Applications that manage state in Kubernetes--for instance, those that
//! update resource statuses, may need to coordinate access to that state so
//! that only one replica is trying to update resources at a time.
//!
//! [`LeaseManager`] interacts with a [`coordv1::Lease`] resource to ensure that
//! only a single claimant owns the lease at a time.

use futures_util::TryFutureExt;
use k8s_openapi::{api::coordination::v1 as coordv1, apimachinery::pkg::apis::meta::v1 as metav1};
use std::{borrow::Cow, sync::Arc};
use tokio::time::{self, Duration};

#[cfg(all(feature = "runtime", feature = "runtime-diagnostics"))]
use crate::admin::LeaseDiagnostics;

/// Manages a Kubernetes `Lease`
#[cfg_attr(docsrs, doc(cfg(feature = "lease")))]
pub struct LeaseManager {
    api: Api,
    name: String,
    field_manager: Cow<'static, str>,
    state: tokio::sync::Mutex<State>,

    #[cfg(all(feature = "runtime", feature = "runtime-diagnostics"))]
    diagnostics: Option<LeaseDiagnostics>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(docsrs, doc(cfg(feature = "lease")))]
/// Configures a Lease.
pub struct LeaseParams {
    /// Lease name.
    pub name: String,

    /// Lease namespace.
    pub namespace: String,

    /// The identity of the claimant.
    pub claimant: String,

    /// The duration of the lease
    pub lease_duration: Duration,

    /// The amount of time before the lease expiration that the lease holder
    /// should renew the lease
    pub renew_grace_period: Duration,

    /// The field manager used when updating the Lease.
    pub field_manager: Option<Cow<'static, str>>,
}

/// Configuration used when obtaining a lease.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(docsrs, doc(cfg(feature = "lease")))]
pub struct ClaimParams {
    /// The duration of the lease
    pub lease_duration: Duration,

    /// The amount of time before the lease expiration that the lease holder
    /// should renew the lease
    pub renew_grace_period: Duration,
}

/// Describes the state of a lease
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "runtime-diagnostics", derive(serde::Serialize))]
#[cfg_attr(docsrs, doc(cfg(feature = "lease")))]
pub struct Claim {
    /// The identity of the claim holder.
    pub holder: String,

    /// The time that the lease expires.
    pub expiry: chrono::DateTime<chrono::Utc>,
}

/// Indicates an error interacting with the Lease API
#[derive(Debug, thiserror::Error)]
#[cfg_attr(docsrs, doc(cfg(feature = "lease")))]
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

    /// A Kubernetes API call timed out
    #[error("timed out")]
    Timeout,
}

#[derive(Clone, Debug)]
struct State {
    meta: Meta,
    claim: Option<Arc<Claim>>,
}

#[derive(Clone, Debug)]
struct Meta {
    version: String,
    transitions: u16,
}

pub(crate) type Api = kube_client::Api<coordv1::Lease>;

pub(crate) type Spawned = (
    tokio::sync::watch::Receiver<Arc<Claim>>,
    tokio::task::JoinHandle<Result<(), Error>>,
);

// === impl ClaimParams ===

impl Default for ClaimParams {
    fn default() -> Self {
        Self {
            lease_duration: Duration::from_secs(30),
            renew_grace_period: Duration::from_secs(1),
        }
    }
}

// === impl Claim ===

impl Claim {
    /// Returns true iff the claim is still valid according to the system clock
    #[inline]
    pub fn is_current(&self) -> bool {
        chrono::Utc::now() < self.expiry
    }

    /// Returns true iff the claim is still valid for the provided claimant
    #[inline]
    pub fn is_current_for(&self, claimant: &str) -> bool {
        self.holder == claimant && self.is_current()
    }

    /// Waits for the claim to expire
    pub async fn expire(&self) {
        self.expire_with_grace(Duration::ZERO).await;
    }

    /// Waits until there is a grace period remaining before the claim expires
    pub async fn expire_with_grace(&self, grace: Duration) {
        if let Ok(remaining) = (self.expiry - chrono::Utc::now()).to_std() {
            let sleep = remaining.saturating_sub(grace);
            if !sleep.is_zero() {
                tokio::time::sleep(sleep).await;
            }
        }
    }
}

// === impl LeaseManager ===

impl LeaseManager {
    pub(crate) const DEFAULT_FIELD_MANAGER: &'static str = "kubert";
    const DEFAULT_MIN_BACKOFF: Duration = Duration::from_millis(5);
    const DEFAULT_BACKOFF_JITTER: f64 = 0.5; // up to 50% of the backoff duration
    const API_TIMEOUT: Duration = Duration::from_secs(10);

    /// Initialize a lease's state from the Kubernetes API.
    ///
    /// The named lease resource must already have been created, or a 404 error
    /// will be returned.
    pub async fn init(api: Api, name: impl ToString) -> Result<Self, Error> {
        let name = name.to_string();
        let state = Self::get(api.clone(), &name).await?;
        Ok(Self {
            api,
            name,
            field_manager: Self::DEFAULT_FIELD_MANAGER.into(),
            state: tokio::sync::Mutex::new(state),
            diagnostics: None,
        })
    }

    /// Overrides the field manager used when updating the Lease
    ///
    /// This is intended to be used immediately following initialization and
    /// before `ensure_claimed` is invoked.
    pub fn with_field_manager(mut self, field_manager: impl Into<Cow<'static, str>>) -> Self {
        self.field_manager = field_manager.into();
        self
    }

    #[cfg(all(feature = "runtime", feature = "runtime-diagnostics"))]
    pub(crate) fn with_diagnostics(mut self, diagnostics: LeaseDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Return the state of the claim without updating it from the API.
    pub async fn claimed(&self) -> Option<Arc<Claim>> {
        self.state.lock().await.claim.clone()
    }

    /// Update the state of the claim from the API.
    pub async fn sync(&self) -> Result<Option<Arc<Claim>>, Error> {
        let mut state = self.state.lock().await;
        *state = Self::get(self.api.clone(), &self.name).await?;
        #[cfg(all(feature = "runtime", feature = "runtime-diagnostics"))]
        if let Some(diagnostics) = self.diagnostics.as_ref() {
            diagnostics.inspect(state.claim.clone(), state.meta.version.clone());
        }
        Ok(state.claim.clone())
    }

    /// Ensures that the lease, if it exists, is claimed.
    ///
    /// If these is not currently held, it is claimed by the provided identity.
    /// If it is currently held by the provided claimant, it is renewed if it is
    /// within the renew grace period.
    pub async fn ensure_claimed(
        &self,
        claimant: &str,
        params: &ClaimParams,
    ) -> Result<Arc<Claim>, Error> {
        let mut state = self.state.lock().await;
        loop {
            if let Some(claim) = state.claim.as_ref() {
                // If the claim is held by the provided claimant, then consider
                // renewing the claim.
                if claim.holder == claimant {
                    let renew_at = claim.expiry
                        - chrono::Duration::from_std(params.renew_grace_period)
                            .unwrap_or_else(|_| chrono::Duration::zero());
                    if chrono::Utc::now() < renew_at {
                        return Ok(claim.clone());
                    }

                    let (claim, meta) = match self.renew(&state.meta, claimant, params).await {
                        Ok(renew) => renew,

                        Err(e) if Self::is_conflict(&e) => {
                            // Another process updated the claim's resource version, so
                            // re-sync the state and try again.
                            *state = Self::get(self.api.clone(), &self.name).await?;
                            #[cfg(all(feature = "runtime", feature = "runtime-diagnostics"))]
                            if let Some(diagnostics) = self.diagnostics.as_ref() {
                                diagnostics
                                    .inspect(state.claim.clone(), state.meta.version.clone());
                            }
                            continue;
                        }

                        Err(e) => return Err(e),
                    };

                    *state = State {
                        claim: Some(claim.clone()),
                        meta,
                    };
                    #[cfg(all(feature = "runtime", feature = "runtime-diagnostics"))]
                    if let Some(diagnostics) = self.diagnostics.as_ref() {
                        diagnostics.inspect(state.claim.clone(), state.meta.version.clone());
                    }
                    return Ok(claim);
                }

                // The claim is held by another claimant, return it.
                if claim.is_current() {
                    return Ok(claim.clone());
                }
            }

            // There's no current claim, so try to acquire it.
            let (claim, meta) = match self.acquire(&state.meta, claimant, params).await {
                Ok(acquire) => acquire,

                Err(e) if Self::is_conflict(&e) => {
                    // Another process updated the claim's resource version, so
                    // re-sync the state and try again.
                    *state = Self::get(self.api.clone(), &self.name).await?;
                    #[cfg(all(feature = "runtime", feature = "runtime-diagnostics"))]
                    if let Some(diagnostics) = self.diagnostics.as_ref() {
                        diagnostics.inspect(state.claim.clone(), state.meta.version.clone());
                    }
                    continue;
                }

                Err(e) => return Err(e),
            };

            *state = State {
                claim: Some(claim.clone()),
                meta,
            };
            #[cfg(all(feature = "runtime", feature = "runtime-diagnostics"))]
            if let Some(diagnostics) = self.diagnostics.as_ref() {
                diagnostics.inspect(state.claim.clone(), state.meta.version.clone());
            }

            return Ok(claim);
        }
    }

    /// Clear out the state of the lease if the claim is currently held by the
    /// provided identity.
    ///
    /// This is typically used during process shutdown so that another process
    /// can potentially claim the lease before the prior lease duration expires.
    pub async fn vacate(&self, claimant: &str) -> Result<bool, Error> {
        let mut state = self.state.lock().await;
        let Some(claim) = state.claim.take() else {
            return Ok(false);
        };

        if !claim.is_current() {
            return Ok(false);
        }

        if claim.holder != claimant {
            state.claim = Some(claim);
            return Ok(false);
        }

        let lease = self
            .patch(&kube_client::api::Patch::Strategic(serde_json::json!({
                "apiVersion": "coordination.k8s.io/v1",
                "kind": "Lease",
                "metadata": {
                    "resourceVersion": state.meta.version,
                },
                "spec": {
                    "acquireTime": Option::<()>::None,
                    "renewTime": Option::<()>::None,
                    "holderIdentity": Option::<()>::None,
                    "leaseDurationSeconds": Option::<()>::None,
                    // leaseTransitions is preserved by strategic patch
                },
            })))
            .await?;

        #[cfg(all(feature = "runtime", feature = "runtime-diagnostics"))]
        if let Some(diagnostics) = self.diagnostics.as_ref() {
            diagnostics.inspect(
                None,
                lease.metadata.resource_version.clone().unwrap_or_default(),
            );
        }

        Ok(true)
    }

    /// Spawn a task that ensures the lease is claimed.
    ///
    /// When the lease becomes unclaimed, the task attempts to claim the lease
    /// as _claimant_ and maintains the lease until the task completes or the
    /// lease is claimed by another process.
    ///
    /// The state of the lease is published via the returned receiver.
    ///
    /// When all receivers are dropped, the task completes and the lease is
    /// vacated so that another process can claim it.
    pub async fn spawn(
        self,
        claimant: impl ToString,
        params: ClaimParams,
    ) -> Result<Spawned, Error> {
        let claimant = claimant.to_string();
        let mut claim = self.ensure_claimed(&claimant, &params).await?;
        let (tx, rx) = tokio::sync::watch::channel(claim.clone());
        let mut new_backoff = backoff::ExponentialBackoffBuilder::default();
        new_backoff
            .with_initial_interval(Self::DEFAULT_MIN_BACKOFF)
            .with_randomization_factor(Self::DEFAULT_BACKOFF_JITTER);

        let task = tokio::spawn(async move {
            loop {
                // The claimant has the privilege of renewing the lease before
                // the claim expires.
                let grace = if claim.holder == claimant {
                    params.renew_grace_period
                } else {
                    Duration::ZERO
                };

                // Wait for the current claim to expire. If all receivers are
                // dropped while we're waiting, the task terminates.
                tokio::select! {
                    biased;
                    _ = tx.closed() => break,
                    _ = claim.expire_with_grace(grace) => {}
                }

                // Update the claim and broadcast it to all receivers.
                let backoff = new_backoff.with_max_interval(grace).build();
                claim = backoff::future::retry(backoff, || {
                    self.ensure_claimed(&claimant, &params).map_err(|err| match err {
                        err @ Error::Api(kube_client::Error::Auth(_))
                        | err @ Error::Api(kube_client::Error::Discovery(_))
                        | err @ Error::Api(kube_client::Error::BuildRequest(_)) => {
                            backoff::Error::Permanent(err)
                        },
                        err @ Error::Api(kube_client::Error::InferConfig(_)) => {
                            debug_assert!(false, "InferConfig errors should only be returned when constructing a new client");
                            backoff::Error::Permanent(err)
                        },
                        // Retry any other API request errors.
                        err => {
                            tracing::debug!(error = %err, "Error claiming lease, retrying...");
                            backoff::Error::Transient {
                                err,
                                // Allow the backoff implementation to select how
                                // long to wait before retrying.
                                retry_after: None,
                            }
                        }
                    })
                })
                .await?;
                if tx.send(claim.clone()).is_err() {
                    // All receivers have been dropped.
                    break;
                }
            }

            self.vacate(&claimant).await?;
            Ok(())
        });

        Ok((rx, task))
    }

    /// Acquire the lease (i.e. assuming the claimant IS NOT the current holder
    /// of the lease).
    ///
    /// A server-side apply is used to update the resource. If another writer
    /// has updated the resource since the last read, this write fails with a
    /// conflict.
    async fn acquire(
        &self,
        meta: &Meta,
        claimant: &str,
        params: &ClaimParams,
    ) -> Result<(Arc<Claim>, Meta), Error> {
        let lease_duration =
            chrono::Duration::from_std(params.lease_duration).unwrap_or(chrono::Duration::MAX);
        let now = chrono::Utc::now();
        let lease = self
            .patch(&kube_client::api::Patch::Apply(serde_json::json!({
                "apiVersion": "coordination.k8s.io/v1",
                "kind": "Lease",
                "metadata": {
                    "resourceVersion": meta.version,
                },
                "spec": {
                    "acquireTime": metav1::MicroTime(now),
                    "renewTime": metav1::MicroTime(now),
                    "holderIdentity": claimant,
                    "leaseDurationSeconds": lease_duration.num_seconds(),
                    "leaseTransitions": meta.transitions + 1,
                },
            })))
            .await?;

        let claim = Claim {
            holder: claimant.to_string(),
            expiry: now + lease_duration,
        };
        let meta = Meta {
            version: lease
                .metadata
                .resource_version
                .ok_or(Error::MissingResourceVersion)?,
            transitions: meta.transitions + 1,
        };
        Ok((claim.into(), meta))
    }

    /// Renew the lease (i.e. assuming the claimant IS the current holder of the
    /// lease).
    ///
    /// A strategic merge is used so that only the `renewTime` field is updated
    /// in most cases. The `leaseDurationSeconds` fields may also be updated if
    /// the caller passed an updated value.
    async fn renew(
        &self,
        meta: &Meta,
        claimant: &str,
        params: &ClaimParams,
    ) -> Result<(Arc<Claim>, Meta), Error> {
        let lease_duration =
            chrono::Duration::from_std(params.lease_duration).unwrap_or(chrono::Duration::MAX);
        let now = chrono::Utc::now();
        let lease = self
            .patch(&kube_client::api::Patch::Strategic(serde_json::json!({
                "apiVersion": "coordination.k8s.io/v1",
                "kind": "Lease",
                "metadata": {
                    "resourceVersion": meta.version,
                },
                "spec": {
                    "renewTime": metav1::MicroTime(now),
                    "leaseDurationSeconds": lease_duration.num_seconds(),
                },
            })))
            .await?;

        let claim = Claim {
            holder: claimant.to_string(),
            expiry: now + lease_duration,
        };
        let meta = Meta {
            version: lease
                .metadata
                .resource_version
                .ok_or(Error::MissingResourceVersion)?,
            transitions: meta.transitions,
        };
        Ok((claim.into(), meta))
    }

    async fn patch<P>(&self, patch: &kube_client::api::Patch<P>) -> Result<coordv1::Lease, Error>
    where
        P: serde::Serialize + std::fmt::Debug,
    {
        tracing::debug!(?patch);
        let params = kube_client::api::PatchParams {
            field_manager: Some(self.field_manager.to_string()),
            // Force conflict resolution when using Server-side Apply (i.e., to
            // acquire a lease). This is the recommended behavior for
            // controllers. See: https://kubernetes.io/docs/reference/using-api/server-side-apply/#conflicts
            force: matches!(patch, kube_client::api::Patch::Apply(_)),
            ..Default::default()
        };
        time::timeout(
            Self::API_TIMEOUT,
            self.api.patch(&self.name, &params, patch),
        )
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(Into::into)
    }

    async fn get(api: Api, name: &str) -> Result<State, Error> {
        let lease = time::timeout(Self::API_TIMEOUT, api.get(name))
            .await
            .map_err(|_| Error::Timeout)??;
        let spec = lease.spec.ok_or(Error::MissingSpec)?;

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

        let metav1::MicroTime(renew_time) = or_unclaimed!(spec.renew_time);
        let lease_duration =
            chrono::Duration::seconds(or_unclaimed!(spec.lease_duration_seconds).into());
        let expiry = renew_time + lease_duration;
        if expiry <= chrono::Utc::now() {
            return Ok(State { meta, claim: None });
        }

        Ok(State {
            meta,
            claim: Some(Arc::new(Claim { holder, expiry })),
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
