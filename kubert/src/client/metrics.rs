use super::svc;
use prometheus_client::{
    metrics::{counter::Counter, family::Family, histogram::Histogram},
    registry::{Registry, Unit},
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::time;

/// Metrics families for a Kubernetes API client.
#[derive(Clone, Debug)]
pub struct ClientMetricsFamilies {
    requests: Family<RequestLabels, Counter>,
    response_latency: Family<RequestLabels, Histogram>,
    response_frames: Family<ResponseStatusLabels, Counter>,
    response_duration: Family<RequestLabels, Histogram>,
}

#[derive(Clone, Debug, Default)]
pub struct ClientMetrics {
    cluster_url: String,
    families: ClientMetricsFamilies,
}

struct ClientMetricsService {
    metrics: ClientMetrics,
    inner: svc::BoxService,
}

struct ResponseBody {
    inner: svc::BoxBody,
    start: Option<time::Instant>,
    response_frames: Counter,
    responses: Histogram,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, prometheus_client::encoding::EncodeLabelSet)]
struct RequestLabels {
    cluster_url: String,
    method: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, prometheus_client::encoding::EncodeLabelSet)]
struct ResponseStatusLabels {
    cluster_url: String,
    method: String,
    status: Option<u16>,
    error: Option<ErrorKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, prometheus_client::encoding::EncodeLabelValue)]
enum ErrorKind {
    Timeout,
    Other,
}

pub fn layer(
    metrics: ClientMetrics,
) -> impl svc::Layer<svc::BoxService, Service = svc::BoxService> {
    svc::layer_fn(move |inner| {
        svc::BoxService::new(ClientMetricsService {
            metrics: metrics.clone(),
            inner,
        })
    })
}

impl ClientMetricsFamilies {
    /// Registers the metrics in the given registry.
    pub fn register(registry: &mut Registry) -> Self {
        let families = Self::default();
        let Self {
            requests,
            response_duration,
            response_frames,
            response_latency,
        } = &families;

        registry.register(
            "requests",
            "Number of requests sent by tha Kubernetes API client",
            requests.clone(),
        );

        registry.register(
            "response_frames",
            "Response frames received by the Kubernetes API client",
            response_frames.clone(),
        );

        registry.register_with_unit(
            "response_latency",
            "Time between a request being dispatched and its response headers being received",
            Unit::Seconds,
            response_latency.clone(),
        );

        registry.register_with_unit(
            "response_duration",
            "Duration of a response stream from receiving the initial status to the end of the stream",
            Unit::Seconds,
            response_duration.clone(),
        );

        families
    }

    pub(super) fn metrics(&self, config: &super::Config) -> ClientMetrics {
        ClientMetrics {
            cluster_url: config.cluster_url.to_string(),
            families: self.clone(),
        }
    }
}

impl Default for ClientMetricsFamilies {
    fn default() -> Self {
        Self {
            requests: Family::default(),
            response_frames: Family::default(),
            response_latency: Family::new_with_constructor(|| {
                // Indicates whether we're getting timely responses or slow
                // responses.
                const BUCKETS: &[f64] = &[0.3, 3.0];
                Histogram::new(BUCKETS.iter().copied())
            }),
            response_duration: Family::new_with_constructor(|| {
                // Demonstrates short-lived responses versus long-lived streams.
                const BUCKETS: &[f64] = &[3.0, 300.0, 1200.0];
                Histogram::new(BUCKETS.iter().copied())
            }),
        }
    }
}

impl svc::Service<svc::Request> for ClientMetricsService {
    type Response = svc::Response;
    type Error = svc::BoxError;
    type Future = svc::BoxFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: svc::Request) -> Self::Future {
        let Self {
            metrics,
            ref mut inner,
        } = self;

        let cluster_url = metrics.cluster_url.clone();
        let method = req.method().as_str().to_string();

        let req_labels = RequestLabels {
            cluster_url: metrics.cluster_url.clone(),
            method: method.clone(),
        };
        metrics.families.requests.get_or_create(&req_labels).inc();

        let response_frames = metrics.families.response_frames.clone();
        let response_latency = metrics
            .families
            .response_latency
            .get_or_create(&req_labels)
            .clone();
        let responses = metrics
            .families
            .response_duration
            .get_or_create(&req_labels)
            .clone();

        let start = time::Instant::now();
        let call = inner.call(req);
        Box::pin(async move {
            let res = call.await;
            let receipt = time::Instant::now();
            response_latency.observe(receipt.saturating_duration_since(start).as_secs_f64());

            let rsp_labels = {
                let status = res.as_ref().ok().map(|res| res.status().as_u16());
                let error = res.as_ref().err().map(|err| {
                    if err.is::<super::timeouts::ResponseHeadersTimeoutError>() {
                        ErrorKind::Timeout
                    } else {
                        ErrorKind::Other
                    }
                });
                ResponseStatusLabels {
                    cluster_url,
                    method,
                    status,
                    error,
                }
            };
            let response_frames = response_frames.get_or_create(&rsp_labels).clone();

            res.map(move |rsp| {
                rsp.map(move |inner| {
                    Box::new(ResponseBody {
                        inner,
                        start: Some(receipt),
                        responses,
                        response_frames,
                    }) as svc::BoxBody
                })
            })
        })
    }
}

impl hyper::body::Body for ResponseBody {
    type Data = bytes::Bytes;
    type Error = svc::BoxError;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let res = futures_util::ready!(Pin::new(&mut this.inner).poll_frame(cx));
        match &res {
            Some(Ok(f)) if f.is_data() => {
                this.response_frames.inc();
            }
            Some(Err(_)) | None => {
                if let Some(start) = this.start.take() {
                    this.responses.observe(start.elapsed().as_secs_f64());
                }
            }
            _ => {}
        }
        Poll::Ready(res)
    }

    #[inline]
    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    #[inline]
    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}
