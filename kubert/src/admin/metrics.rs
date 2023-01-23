use super::*;

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
            hyper::Method::GET | hyper::Method::HEAD => Response::builder()
                .status(hyper::StatusCode::OK)
                .header(hyper::header::CONTENT_TYPE, "text/plain")
                .body(self.metrics.render().into())
                .unwrap(),
            _ => Response::builder()
                .status(hyper::StatusCode::METHOD_NOT_ALLOWED)
                .header(hyper::header::ALLOW, "GET, HEAD")
                .body(Body::default())
                .unwrap(),
        }
    }
}

impl fmt::Debug for Prometheus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Prometheus")
            .field("metrics", &format_args!("..."))
            .field("process", &self.process)
            .finish()
    }
}
