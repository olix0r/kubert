use ahash::AHashMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube_runtime::watcher;
use parking_lot::{Mutex, RwLock};
use std::sync::{Arc, Weak};

#[derive(Clone, Debug, Default)]
pub(crate) struct Diagnostics {
    watches: Arc<Mutex<Vec<Weak<RwLock<WatchState>>>>>,
}

pub(crate) struct WatchDiagnostics(Arc<RwLock<WatchState>>);

#[derive(Clone, Debug)]
pub struct WatchState {
    api_url: String,
    label_selector: String,
    stats: WatchStats,
    known: AHashMap<ObjRef, Resource>,
    resetting: AHashMap<ObjRef, Resource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ObjRef {
    kind: String,
    api_version: String,
    namespace: Option<String>,
    name: Option<String>,
    uid: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct Summary {
    watches: Vec<WatchSummary>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct WatchSummary {
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
pub(crate) struct WatchStats {
    created_at: chrono::DateTime<chrono::Utc>,

    resets: u64,
    errors: u64,
    applies: u64,
    deletes: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<WatchError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_reset: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_apply: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_delete: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct WatchError {
    message: String,
    time: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
struct Resource {
    creation_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    name: String,
    namespace: String,
    resource_version: String,
    uid: String,
}

// === impl Diagnostics ===

impl Diagnostics {
    pub(crate) fn summarize(&self, with_resources: bool) -> Summary {
        // Collect the summaries of the remaining watches, with their resources
        // sorted by creation.
        let watches = {
            let mut refs = self.watches.lock();
            // Clean up any dead weak refs, i.e. of watches that have been dropped.
            refs.retain(|w| w.upgrade().is_some());
            refs.iter()
                .filter_map(|wref| {
                    let watch = wref.upgrade()?;
                    let state = watch.read();

                    let mut resources = state.known.values().cloned().collect::<Vec<_>>();
                    resources.sort_by_key(|meta| meta.creation_timestamp);

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

                    Some(WatchSummary {
                        api_url: state.api_url.clone(),
                        label_selector: state.label_selector.clone(),
                        stats: state.stats.clone(),
                        resources,
                        checksum,
                    })
                })
                .collect()
        };

        Summary { watches }
    }

    pub(crate) fn register_watch<T>(
        &self,
        api: &super::Api<T>,
        label_selector: Option<&str>,
    ) -> WatchDiagnostics
    where
        T: kube_client::Resource,
        T::DynamicType: Default,
    {
        let now = chrono::Utc::now();
        let state = Arc::new(RwLock::new(WatchState {
            api_url: api.resource_url().to_string(),
            label_selector: label_selector.map_or_else(Default::default, ToString::to_string),
            known: AHashMap::new(),
            resetting: AHashMap::new(),
            stats: WatchStats {
                created_at: now,
                resets: 0,
                errors: 0,
                applies: 0,
                deletes: 0,
                last_error: None,
                last_reset: None,
                last_apply: None,
                last_delete: None,
            },
        }));

        let watch = Arc::downgrade(&state);
        self.watches.lock().push(watch);

        WatchDiagnostics(state)
    }
}

impl WatchDiagnostics {
    pub(crate) fn inspect<T>(&self, event: &watcher::Result<watcher::Event<T>>)
    where
        T: kube_client::Resource,
        T::DynamicType: Default,
    {
        let obj_ref = |meta: &ObjectMeta| ObjRef {
            kind: T::kind(&Default::default()).to_string(),
            api_version: T::api_version(&Default::default()).to_string(),
            namespace: meta.namespace.clone(),
            name: meta.name.clone(),
            uid: meta.uid.clone(),
        };
        let prep_meta = |meta: &ObjectMeta| Resource {
            creation_timestamp: meta.creation_timestamp.clone().map(|t| t.0),
            name: meta.name.clone().unwrap_or_default(),
            namespace: meta.namespace.clone().unwrap_or_default(),
            resource_version: meta.resource_version.clone().unwrap_or_default(),
            uid: meta.uid.clone().unwrap_or_default(),
        };

        let WatchState {
            ref mut known,
            ref mut resetting,
            ref mut stats,
            ..
        } = *self.0.write();
        let now = chrono::Utc::now();

        match event {
            Ok(watcher::Event::Init) => {
                resetting.clear();
            }
            Ok(watcher::Event::InitApply(res)) => {
                resetting.insert(obj_ref(res.meta()), prep_meta(res.meta()));
            }
            Ok(watcher::Event::InitDone) => {
                std::mem::swap(known, resetting);
                stats.resets += 1;
                stats.last_reset = Some(now);
            }
            Ok(watcher::Event::Apply(res)) => {
                known.insert(obj_ref(res.meta()), prep_meta(res.meta()));
                stats.applies += 1;
                stats.last_apply = Some(now);
            }
            Ok(watcher::Event::Delete(res)) => {
                known.remove(&obj_ref(res.meta()));
                stats.deletes += 1;
                stats.last_delete = Some(now);
            }
            Err(error) => {
                stats.errors += 1;
                stats.last_error = Some(WatchError {
                    message: error.to_string(),
                    time: now,
                });
            }
        }
    }
}

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
