use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use parking_lot::RwLock;
use std::{
    borrow::Cow,
    sync::{Arc, Weak},
};

pub(crate) struct LeaseDiagnostics(Arc<RwLock<LeaseState>>);

pub(super) type StateRef = Weak<RwLock<LeaseState>>;

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LeaseState {
    name: String,
    namespace: String,
    claimant: String,
    lease_duration_seconds: f64,
    renew_grace_period_seconds: f64,
    field_manager: Cow<'static, str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    claim: Option<crate::lease::Claim>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource_version: Option<String>,
    stats: LeaseStats,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LeaseStats {
    creation_timestamp: Time,

    updates: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    last_update_timestamp: Option<Time>,
}

// === impl LeaseDiagnostics ===

impl LeaseDiagnostics {
    pub(super) fn new(
        crate::LeaseParams {
            name,
            namespace,
            claimant,
            lease_duration,
            renew_grace_period,
            field_manager,
        }: &crate::LeaseParams,
    ) -> Self {
        let now = Time(chrono::Utc::now());
        Self(Arc::new(RwLock::new(LeaseState {
            name: name.clone(),
            namespace: namespace.clone(),
            claimant: claimant.clone(),
            lease_duration_seconds: lease_duration.as_secs_f64(),
            renew_grace_period_seconds: renew_grace_period.as_secs_f64(),
            field_manager: field_manager.clone().unwrap_or(Cow::Borrowed(
                crate::lease::LeaseManager::DEFAULT_FIELD_MANAGER,
            )),
            claim: None,
            resource_version: None,
            stats: LeaseStats {
                creation_timestamp: now,
                updates: 0,
                last_update_timestamp: None,
            },
        })))
    }

    pub(super) fn weak(&self) -> Weak<RwLock<LeaseState>> {
        Arc::downgrade(&self.0)
    }

    pub(crate) fn inspect(
        &self,
        claim: Option<Arc<crate::lease::Claim>>,
        resource_version: String,
    ) {
        let mut state = self.0.write();
        if claim.as_deref() == state.claim.as_ref()
            && Some(&*resource_version) == state.resource_version.as_deref()
        {
            return;
        }
        let now = Time(chrono::Utc::now());
        state.claim = claim.as_deref().cloned();
        state.resource_version = Some(resource_version);
        state.stats.updates += 1;
        state.stats.last_update_timestamp = Some(now);
    }
}
