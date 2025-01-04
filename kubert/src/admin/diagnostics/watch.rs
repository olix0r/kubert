use ahash::AHashMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
use kube_runtime::watcher;
use parking_lot::RwLock;
use std::sync::{Arc, Weak};

pub(crate) struct WatchDiagnostics(Arc<RwLock<WatchState>>);

pub(super) type StateRef = Weak<RwLock<WatchState>>;

#[derive(Clone, Debug)]
pub(super) struct WatchState {
    api_url: String,
    label_selector: String,
    stats: WatchStats,
    known: AHashMap<ObjRef, Resource>,
    resetting: AHashMap<ObjRef, Resource>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct WatchSummary {
    api_url: String,
    label_selector: String,
    #[serde(flatten)]
    stats: WatchStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resources: Option<Vec<Resource>>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct WatchStats {
    creation_timestamp: Time,

    errors: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<WatchError>,

    resets: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_reset_timestamp: Option<Time>,

    applies: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_apply_timestamp: Option<Time>,

    deletes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_delete_timestamp: Option<Time>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct WatchError {
    message: String,
    timestamp: Time,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ObjRef {
    kind: String,
    api_version: String,
    namespace: Option<String>,
    name: Option<String>,
    uid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct Resource {
    creation_timestamp: Option<Time>,
    uid: String,
    name: String,
    namespace: String,
    generation: Option<i64>,
    resource_version: String,
}

// === impl WatchDiagnostics ===

impl WatchDiagnostics {
    pub(super) fn new(api_url: &str, label_selector: Option<&str>) -> Self {
        Self(Arc::new(RwLock::new(WatchState {
            api_url: api_url.to_string(),
            label_selector: label_selector.unwrap_or_default().to_string(),
            stats: WatchStats {
                creation_timestamp: Time(chrono::Utc::now()),
                errors: 0,
                last_error: None,
                resets: 0,
                last_reset_timestamp: None,
                applies: 0,
                last_apply_timestamp: None,
                deletes: 0,
                last_delete_timestamp: None,
            },
            known: AHashMap::new(),
            resetting: AHashMap::new(),
        })))
    }

    pub(super) fn weak(&self) -> Weak<RwLock<WatchState>> {
        Arc::downgrade(&self.0)
    }
}

// === impl WatchDiagnostics ===

impl WatchDiagnostics {
    pub(crate) fn inspect<T>(&self, event: &watcher::Result<watcher::Event<T>>)
    where
        T: kube_client::Resource,
        T::DynamicType: Default,
    {
        let to_key = |meta: &ObjectMeta| ObjRef {
            kind: T::kind(&Default::default()).to_string(),
            api_version: T::api_version(&Default::default()).to_string(),
            namespace: meta.namespace.clone(),
            name: meta.name.clone(),
            uid: meta.uid.clone(),
        };

        // We store a summarized version fo resources to avoid storing, for
        // example, all state for a cluster. We store only the metadata that we
        // can use to establish a comparison between multiple controller
        // instances and the kubernets API state.
        let to_resource = |meta: &ObjectMeta| Resource {
            creation_timestamp: meta.creation_timestamp.clone(),
            name: meta.name.clone().unwrap_or_default(),
            namespace: meta.namespace.clone().unwrap_or_default(),
            resource_version: meta.resource_version.clone().unwrap_or_default(),
            generation: meta.generation,
            uid: meta.uid.clone().unwrap_or_default(),
        };

        let now = Time(chrono::Utc::now());
        let WatchState {
            ref mut known,
            ref mut resetting,
            ref mut stats,
            ..
        } = *self.0.write();
        match event {
            Ok(watcher::Event::Init) => {
                resetting.clear();
            }
            Ok(watcher::Event::InitApply(res)) => {
                resetting.insert(to_key(res.meta()), to_resource(res.meta()));
            }
            Ok(watcher::Event::InitDone) => {
                std::mem::swap(known, resetting);
                stats.resets += 1;
                stats.last_reset_timestamp = Some(now);
            }
            Ok(watcher::Event::Apply(res)) => {
                known.insert(to_key(res.meta()), to_resource(res.meta()));
                stats.applies += 1;
                stats.last_apply_timestamp = Some(now);
            }
            Ok(watcher::Event::Delete(res)) => {
                known.remove(&to_key(res.meta()));
                stats.deletes += 1;
                stats.last_delete_timestamp = Some(now);
            }
            Err(error) => {
                stats.errors += 1;
                stats.last_error = Some(WatchError {
                    message: error.to_string(),
                    timestamp: now,
                });
            }
        }
    }
}

// === impl WatchState ===

impl WatchState {
    pub(super) fn summary(&self, with_resources: bool) -> WatchSummary {
        let mut resources = self.known.values().cloned().collect::<Vec<_>>();
        resources.sort_by_key(|meta| meta.creation_timestamp.as_ref().map(|Time(t)| *t));

        let checksum = if resources.is_empty() {
            None
        } else {
            Some(checksum(&resources))
        };
        let resources = if with_resources {
            Some(resources)
        } else {
            None
        };

        WatchSummary {
            api_url: self.api_url.clone(),
            label_selector: self.label_selector.clone(),
            stats: self.stats.clone(),
            resources,
            checksum,
        }
    }
}

// === impl Resource ===

impl std::hash::Hash for Resource {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.creation_timestamp
            .as_ref()
            .map(|Time(ct)| ct)
            .hash(state);
        self.name.hash(state);
        self.namespace.hash(state);
        self.resource_version.hash(state);
        self.uid.hash(state);
    }
}

/// Compute a SHA256 checksum of a hashable object.
fn checksum<T: std::hash::Hash>(obj: &T) -> String {
    use sha2::{Digest, Sha256};
    struct Sha256Hasher(Sha256);
    impl std::hash::Hasher for Sha256Hasher {
        fn finish(&self) -> u64 {
            unimplemented!("SHA-256 output is larger than u64");
        }
        fn write(&mut self, bytes: &[u8]) {
            self.0.update(bytes);
        }
    }
    let mut hasher = Sha256Hasher(Sha256::new());
    obj.hash(&mut hasher);
    format!("sha256:{:x}", hasher.0.finalize())
}
