use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use parking_lot::Mutex;
use std::{net::SocketAddr, sync::Arc};

#[cfg(feature = "lease")]
mod lease;
mod watch;

#[cfg(feature = "lease")]
pub(crate) use self::lease::LeaseDiagnostics;
pub(crate) use self::watch::WatchDiagnostics;

#[derive(Clone, Debug)]
pub(crate) struct Diagnostics {
    initial_time: chrono::DateTime<chrono::Utc>,
    leases: Arc<Mutex<Vec<lease::StateRef>>>,
    watches: Arc<Mutex<Vec<watch::StateRef>>>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    initial_timestamp: Time,
    current_timestamp: Time,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    watches: Vec<watch::WatchSummary>,

    #[cfg(feature = "lease")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    leases: Vec<lease::LeaseState>,
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
        let leases = self.summarize_leases();
        let summary = Summary {
            initial_timestamp: Time(self.initial_time),
            current_timestamp: Time(chrono::Utc::now()),
            watches,
            leases,
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
    fn summarize_watches(&self, with_resources: bool) -> Vec<watch::WatchSummary> {
        let mut refs = self.watches.lock();
        // Clean up any dead weak refs, i.e. of watches that have been dropped.
        refs.retain(|w| w.upgrade().is_some());
        refs.iter()
            .filter_map(|wref| {
                let watch = wref.upgrade()?;
                let state = watch.read();
                Some(state.summary(with_resources))
            })
            .collect()
    }

    fn summarize_leases(&self) -> Vec<lease::LeaseState> {
        let mut refs = self.leases.lock();
        // Clean up any dead weak refs, i.e. of leases that have been dropped.
        refs.retain(|w| w.upgrade().is_some());
        refs.iter()
            .filter_map(|wref| {
                let lease = wref.upgrade()?;
                let state = lease.read();
                Some(state.clone())
            })
            .collect()
    }
}
