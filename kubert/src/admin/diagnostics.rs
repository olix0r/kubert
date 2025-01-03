use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub(crate) struct Diagnostics {
    initial_time: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Summary {
    initial_timestamp: Time,
    current_timestamp: Time,
}

// === impl Diagnostics ===

impl Diagnostics {
    pub(super) fn new() -> Self {
        Self {
            initial_time: chrono::Utc::now(),
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

        let summary = Summary {
            initial_timestamp: Time(self.initial_time),
            current_timestamp: Time(chrono::Utc::now()),
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
}
