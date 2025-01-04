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
    field_manager: Cow<'static, str>,
    lease_duration_seconds: f64,
    renew_grace_period_seconds: f64,
    #[serde(flatten)]
    stats: LeaseStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<Claim>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LeaseStats {
    updates: u64,
    creation_timestamp: Time,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_update_timestamp: Option<Time>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct Claim {
    holder: String,
    expiry: Time,
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
            current: None,
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
        if state.current.as_ref().map(|c| (&c.holder, c.expiry.0))
            == claim.as_deref().map(|c| (&c.holder, c.expiry))
            && Some(&*resource_version) == state.resource_version.as_deref()
        {
            return;
        }
        let now = Time(chrono::Utc::now());
        state.current = claim
            .as_deref()
            .cloned()
            .map(|crate::lease::Claim { holder, expiry }| Claim {
                holder,
                expiry: Time(expiry),
            });
        state.resource_version = Some(resource_version);
        state.stats.updates += 1;
        state.stats.last_update_timestamp = Some(now);
    }
}
