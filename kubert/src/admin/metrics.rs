use super::*;

use hyper::header;
pub(super) use metrics_exporter_prometheus::PrometheusBuilder;
use metrics_exporter_prometheus::PrometheusHandle;
use metrics_process::Collector;

use std::fmt;

#[derive(Clone)]
pub(super) struct Prometheus {
    metrics: PrometheusHandle,
    process: Collector,
}

impl Prometheus {
    pub(super) fn new(builder: PrometheusBuilder) -> Self {
        let metrics = builder
            .install_recorder()
            .expect("failed to install Prometheus recorder");
        let process = Collector::default();
        process.describe();
        Self { metrics, process }
    }

    pub(super) fn handle_metrics(&self, req: Request<Body>) -> Response<Body> {
        self.process.collect();
        match *req.method() {
            hyper::Method::GET | hyper::Method::HEAD => {
                let mut rsp = Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/plain");

                let metrics = self.metrics.render();
                // if the requestor accepts gzip compression, compress the metrics.
                let body = if accepts_gzip(req.headers()) {
                    // XXX(eliza): it's a shame we can't have the `PrometheusHandle`
                    // format the metrics into a writer, rather than into a
                    // string...if we could, we could write directly to the gzip
                    // writer and not have to double-allocate in that case.
                    rsp = rsp.header(header::CONTENT_ENCODING, "gzip");
                    deflate::deflate_bytes_gzip(metrics.as_bytes()).into()
                } else {
                    metrics.into()
                };

                rsp.body(body).unwrap()
            }
            _ => Response::builder()
                .status(hyper::StatusCode::METHOD_NOT_ALLOWED)
                .header(header::ALLOW, "GET, HEAD")
                .body(Body::default())
                .unwrap(),
        }
    }
}

impl fmt::Debug for Prometheus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Prometheus")
            .field("metrics", &format_args!("PrometheusHandle { ... }"))
            .field("process", &self.process)
            .finish()
    }
}

fn accepts_gzip(headers: &header::HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .any(|value| {
            value
                .to_str()
                .ok()
                .map(|value| value.contains("gzip"))
                .unwrap_or(false)
        })
}
