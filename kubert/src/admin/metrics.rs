use super::*;

use hyper::header;
pub use metrics_exporter_prometheus::{BuildError, Matcher};
use metrics_exporter_prometheus::{PrometheusBuilder as InnerBuilder, PrometheusHandle};
use metrics_process::Collector;
pub use metrics_util::MetricKindMask;

use std::{fmt, time::Duration};

#[derive(Default)]
#[cfg_attr(docsrs, doc(cfg(all(feature = "admin", feature = "metrics"))))]
pub struct PrometheusBuilder {
    inner: InnerBuilder,
}

#[derive(Clone)]
pub(super) struct Prometheus {
    metrics: PrometheusHandle,
    process: Collector,
}

// === impl PrometheusBuilder ===

impl PrometheusBuilder {
    /// Creates a new [`PrometheusBuilder`].
    pub fn new() -> Self {
        Self {
            inner: InnerBuilder::new(),
        }
    }

    /// Sets the quantiles to use when rendering histograms.
    ///
    /// Quantiles represent a scale of 0 to 1, where percentiles represent a scale of 1 to 100, so
    /// a quantile of 0.99 is the 99th percentile, and a quantile of 0.99 is the 99.9th percentile.
    ///
    /// Defaults to a hard-coded set of quantiles: 0.0, 0.5, 0.9, 0.95, 0.99, 0.999, and 1.0. This means
    /// that all histograms will be exposed as Prometheus summaries.
    ///
    /// If buckets are set (via [`set_buckets`][Self::set_buckets] or
    /// [`set_buckets_for_metric`][Self::set_buckets_for_metric]) then all histograms will be exposed
    /// as summaries instead.
    ///
    /// ## Errors
    ///
    /// If `quantiles` is empty, an error variant will be thrown.
    pub fn set_quantiles(mut self, quantiles: &[f64]) -> Result<Self, BuildError> {
        self.try_map(|inner| inner.set_quantiles(quantiles))
    }

    /// Sets the buckets to use when rendering histograms.
    ///
    /// Buckets values represent the higher bound of each buckets.  If buckets are set, then all
    /// histograms will be rendered as true Prometheus histograms, instead of summaries.
    ///
    /// ## Errors
    ///
    /// If `values` is empty, an error variant will be thrown.
    pub fn set_buckets(mut self, values: &[f64]) -> Result<Self, BuildError> {
        self.try_map(|inner| inner.set_buckets(values))
    }

    /// Sets the bucket for a specific pattern.
    ///
    /// The match pattern can be a full match (equality), prefix match, or suffix match.  The
    /// matchers are applied in that order if two or more matchers would apply to a single metric.
    /// That is to say, if a full match and a prefix match applied to a metric, the full match would
    /// win, and if a prefix match and a suffix match applied to a metric, the prefix match would win.
    ///
    /// Buckets values represent the higher bound of each buckets.  If buckets are set, then any
    /// histograms that match will be rendered as true Prometheus histograms, instead of summaries.
    ///
    /// This option changes the observer's output of histogram-type metric into summaries.
    /// It only affects matching metrics if [`set_buckets`][Self::set_buckets] was not used.
    ///
    /// ## Errors
    ///
    /// If `values` is empty, an error variant will be thrown.
    pub fn set_buckets_for_metric(
        mut self,
        matcher: Matcher,
        values: &[f64],
    ) -> Result<Self, BuildError> {
        self.try_map(|inner| inner.set_buckets_for_metric(matcher, values))
    }

    /// Sets the idle timeout for metrics.
    ///
    /// If a metric hasn't been updated within this timeout, it will be removed from the registry
    /// and in turn removed from the normal scrape output until the metric is emitted again.  This
    /// behavior is driven by requests to generate rendered output, and so metrics will not be
    /// removed unless a request has been made recently enough to prune the idle metrics.
    ///
    /// Further, the metric kind "mask" configures which metrics will be considered by the idle
    /// timeout.  If the kind of a metric being considered for idle timeout is not of a kind
    /// represented by the mask, it will not be affected, even if it would have othered been removed
    /// for exceeding the idle timeout.
    ///
    /// Refer to the documentation for [`MetricKindMask`](metrics_util::MetricKindMask) for more
    /// information on defining a metric kind mask.
    #[must_use]
    pub fn idle_timeout(mut self, mask: MetricKindMask, timeout: Option<Duration>) -> Self {
        Self {
            inner: self.inner.idle_timeout(mask, timeout),
        }
    }

    /// Adds a global label to this exporter.
    ///
    /// Global labels are applied to all metrics.  Labels defined on the metric key itself have precedence
    /// over any global labels.  If this method is called multiple times, the latest value for a given label
    /// key will be used.
    #[must_use]
    pub fn add_global_label<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            inner: self.inner.add_global_label(key, value),
        }
    }

    fn try_map(
        self,
        f: impl FnOnce(InnerBuilder) -> Result<InnerBuilder, BuildError>,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            inner: f(self.inner)?,
        })
    }
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
            .field("metrics", &format_args!("PrometheusHandle {{ ... }}"))
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
