use ahash::AHashMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
use kube_runtime::watcher;
use parking_lot::{Mutex, RwLock};
use std::{
    net::SocketAddr,
    sync::{Arc, Weak},
};

#[derive(Clone, Debug)]
pub(crate) struct Diagnostics {
    initial_time: chrono::DateTime<chrono::Utc>,
    watches: Arc<Mutex<Vec<Weak<RwLock<WatchState>>>>>,
}

pub(crate) struct WatchDiagnostics(Arc<RwLock<WatchState>>);

#[derive(Clone, Debug)]
struct WatchState {
    api_url: String,
    label_selector: String,
    stats: WatchStats,
    known: AHashMap<ObjRef, Resource>,
    resetting: AHashMap<ObjRef, Resource>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct Summary {
    initial_timestamp: Time,
    current_timestamp: Time,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    watches: Vec<WatchSummary>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct WatchSummary {
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
    name: String,
    namespace: String,
    resource_version: String,
    uid: String,
}

// === impl Diagnostics ===

impl Diagnostics {
    pub(super) fn new() -> Self {
        Self {
            initial_time: chrono::Utc::now(),
            watches: Default::default(),
        }
    }

    pub(super) fn handle(&self, client_addr: SocketAddr, req: super::Request) -> super::Response {
        if req.method() != hyper::Method::GET {
            return hyper::Response::builder()
                .status(hyper::StatusCode::METHOD_NOT_ALLOWED)
                .header(hyper::header::ALLOW, "GET")
                .body(super::Body::default())
                .unwrap();
        }

        if !client_addr.ip().is_loopback() {
            tracing::info!(client.ip=%client_addr.ip(), "Rejecting non-loopback request for diagnostics");
            return hyper::Response::builder()
                .status(hyper::StatusCode::FORBIDDEN)
                .body(super::Body::default())
                .unwrap();
        }

        let with_resources = req.uri().query() == Some("resources");
        let watches = self.summarize_watches(with_resources);
        let summary = Summary {
            initial_timestamp: Time(self.initial_time),
            current_timestamp: Time(chrono::Utc::now()),
            watches,
        };

        let mut bytes = Vec::with_capacity(8 * 1024);
        if let Err(error) = serde_json::to_writer_pretty(&mut bytes, &summary) {
            tracing::error!(%error, "Failed to serialize runtime diagnostics");
            return hyper::Response::builder()
                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                .body(super::Body::default())
                .unwrap();
        }

        hyper::Response::builder()
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(super::Body::from(bytes))
            .unwrap()
    }

    /// Collect the summaries of the remaining watches, with their resources
    /// sorted by creation.
    fn summarize_watches(&self, with_resources: bool) -> Vec<WatchSummary> {
        let mut refs = self.watches.lock();
        // Clean up any dead weak refs, i.e. of watches that have been dropped.
        refs.retain(|w| w.upgrade().is_some());
        refs.iter()
            .filter_map(|wref| {
                let watch = wref.upgrade()?;
                let state = watch.read();

                let mut resources = state.known.values().cloned().collect::<Vec<_>>();
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

                Some(WatchSummary {
                    api_url: state.api_url.clone(),
                    label_selector: state.label_selector.clone(),
                    stats: state.stats.clone(),
                    resources,
                    checksum,
                })
            })
            .collect()
    }

    pub(crate) fn register_watch<T>(
        &self,
        api: &crate::runtime::Api<T>,
        label_selector: Option<&str>,
    ) -> WatchDiagnostics
    where
        T: kube_client::Resource,
        T::DynamicType: Default,
    {
        let now = Time(chrono::Utc::now());
        let state = Arc::new(RwLock::new(WatchState {
            api_url: api.resource_url().to_string(),
            label_selector: label_selector.map_or_else(Default::default, ToString::to_string),
            known: AHashMap::new(),
            resetting: AHashMap::new(),
            stats: WatchStats {
                creation_timestamp: now,
                errors: 0,
                last_error: None,
                resets: 0,
                last_reset_timestamp: None,
                applies: 0,
                last_apply_timestamp: None,
                deletes: 0,
                last_delete_timestamp: None,
            },
        }));

        let watch = Arc::downgrade(&state);
        self.watches.lock().push(watch);

        WatchDiagnostics(state)
    }
}

// === impl WatchDiagnostics ===

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
            creation_timestamp: meta.creation_timestamp.clone(),
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
        let now = Time(chrono::Utc::now());

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
                stats.last_reset_timestamp = Some(now);
            }
            Ok(watcher::Event::Apply(res)) => {
                known.insert(obj_ref(res.meta()), prep_meta(res.meta()));
                stats.applies += 1;
                stats.last_apply_timestamp = Some(now);
            }
            Ok(watcher::Event::Delete(res)) => {
                known.remove(&obj_ref(res.meta()));
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
