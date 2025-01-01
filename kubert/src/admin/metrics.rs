use super::*;

use hyper::header;

#[derive(Clone, Debug)]
pub(super) struct Prometheus {
    registry: Arc<prometheus_client::registry::Registry>,
}

impl Prometheus {
    pub(super) fn new(reg: prometheus_client::registry::Registry) -> Self {
        Self {
            registry: reg.into(),
        }
    }

    pub(super) fn handle_metrics(&self, req: Request<hyper::body::Incoming>) -> Response<Body> {
        if !matches!(*req.method(), hyper::Method::GET | hyper::Method::HEAD) {
            return Response::builder()
                .status(hyper::StatusCode::METHOD_NOT_ALLOWED)
                .header(header::ALLOW, "GET, HEAD")
                .body(Body::default())
                .unwrap();
        }

        let body = match self.encode_body() {
            Ok(body) => body,
            Err(error) => {
                tracing::error!(%error, "Failed to encode metrics");
                return Response::builder()
                    .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::default())
                    .unwrap();
            }
        };

        const OPENMETRICS_CONTENT_TYPE: &str =
            "application/openmetrics-text; version=1.0.0; charset=utf-8";
        Response::builder()
            .status(hyper::StatusCode::OK)
            .header(header::CONTENT_TYPE, OPENMETRICS_CONTENT_TYPE)
            .body(body)
            .expect("response must be valid")
    }

    fn encode_body(&self) -> std::result::Result<super::Body, std::fmt::Error> {
        let mut buf = String::with_capacity(16 * 1024);
        prometheus_client::encoding::text::encode(&mut buf, &self.registry)?;
        Ok(super::Body::new(buf.into()))
    }
}
