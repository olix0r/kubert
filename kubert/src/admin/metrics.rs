use hyper::{header, Body, Request, Response, StatusCode};
use ipnet::IpNet;
pub use metrics_exporter_prometheus::{BuildError, Matcher};
use metrics_exporter_prometheus::{PrometheusBuilder as InnerBuilder, PrometheusHandle};
use metrics_process::Collector;
pub use metrics_util::MetricKindMask;

use std::{fmt, net::IpAddr, sync::Arc, time::Duration};

#[derive(Default)]
#[cfg_attr(docsrs, doc(cfg(all(feature = "admin", feature = "metrics"))))]
pub struct PrometheusBuilder {
    inner: InnerBuilder,
    allowed_nets: Option<Vec<IpNet>>,
}

#[derive(Clone)]
pub(super) struct Prometheus {
    metrics: PrometheusHandle,
    process: Collector,
    allowed_nets: Option<Arc<[IpNet]>>,
}

// === impl PrometheusBuilder ===

impl PrometheusBuilder {
    /// Creates a new [`PrometheusBuilder`].
    pub fn new() -> Self {
        Self {
            inner: InnerBuilder::new(),
            allowed_nets: None,
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
    pub fn set_quantiles(self, quantiles: &[f64]) -> Result<Self, BuildError> {
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
    pub fn set_buckets(self, values: &[f64]) -> Result<Self, BuildError> {
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
        self,
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
    pub fn idle_timeout(self, mask: MetricKindMask, timeout: Option<Duration>) -> Self {
        Self {
            inner: self.inner.idle_timeout(mask, timeout),
            ..self
        }
    }

    /// Adds a global label to this exporter.
    ///
    /// Global labels are applied to all metrics.  Labels defined on the metric key itself have precedence
    /// over any global labels.  If this method is called multiple times, the latest value for a given label
    /// key will be used.
    #[must_use]
    pub fn add_global_label<K, V>(self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            inner: self.inner.add_global_label(key, value),
            ..self
        }
    }
    /// Adds an IP address or subnet to the allowlist for the scrape endpoint.
    ///
    /// If a client makes a request to the scrape endpoint and their IP is not present in the
    /// allowlist, either directly or within any of the allowed subnets, they will receive a 403
    /// Forbidden response.
    ///
    /// Defaults to allowing all IPs.
    ///
    /// ## Security Considerations
    ///
    /// On its own, an IP allowlist is insufficient for access control, if the exporter is running
    /// in an environment alongside applications (such as web browsers) that are susceptible to [DNS
    /// rebinding](https://en.wikipedia.org/wiki/DNS_rebinding) attacks.
    ///
    /// ## Errors
    ///
    /// If the given address cannot be parsed into an IP address or subnet, an error variant will be
    /// returned describing the error.
    pub fn add_allowed_address<A>(mut self, address: A) -> Result<Self, BuildError>
    where
        A: AsRef<str>,
    {
        use std::str::FromStr;

        let address = IpNet::from_str(address.as_ref())
            .map_err(|e| BuildError::InvalidAllowlistAddress(e.to_string()))?;
        self.allowed_nets.get_or_insert_with(Vec::new).push(address);

        Ok(self)
    }

    fn try_map(
        self,
        f: impl FnOnce(InnerBuilder) -> Result<InnerBuilder, BuildError>,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            inner: f(self.inner)?,
            ..self
        })
    }
}

impl fmt::Debug for PrometheusBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrometheusBuilder")
            // this type from `metrics-exporter-prometheus` doesn't implement `Debug`...
            .field("inner", &format_args!("PrometheusBuilder {{ ... }}"))
            .field("allowed_nets", &self.allowed_nets)
            .finish()
    }
}

// === impl Prometheus ===

impl Prometheus {
    pub(super) fn new(builder: PrometheusBuilder) -> Self {
        let metrics = builder
            .inner
            .install_recorder()
            .expect("failed to install Prometheus recorder");
        let allowed_nets = builder.allowed_nets.map(Into::into);
        let process = Collector::default();
        process.describe();
        Self {
            metrics,
            process,
            allowed_nets,
        }
    }

    pub(super) fn handle_metrics(&self, remote_ip: IpAddr, req: Request<Body>) -> Response<Body> {
        // If the allowlist is empty, the request is allowed.  Otherwise, it must
        // match one of the entries in the allowlist or it will be denied.
        let allowed = self.allowed_nets.as_ref().map_or(true, |addresses| {
            addresses.iter().any(|address| address.contains(&remote_ip))
        });

        if !allowed {
            tracing::info!("Denying metrics scrape from {remote_ip}; address not in allowlist");
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::default())
                .unwrap();
        }

        self.process.collect();
        match *req.method() {
            hyper::Method::GET | hyper::Method::HEAD => {
                let mut rsp = Response::builder()
                    .status(StatusCode::OK)
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
                .status(StatusCode::METHOD_NOT_ALLOWED)
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
            .field("allowed_nets", &self.allowed_nets)
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
