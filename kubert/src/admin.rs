//! Admin server utilities.
use futures_util::future;
use hyper::{Body, Request, Response};

use std::{
    fmt,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{debug, info_span, Instrument};

/// Results that may fail with a server error
pub type Result<T> = hyper::Result<T>;

/// Server errors
pub type Error = hyper::Error;

#[cfg(feature = "metrics")]
mod metrics;

/// Command-line arguments used to configure an admin server
#[derive(Clone, Debug)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
pub struct AdminArgs {
    /// The admin server's address
    #[cfg_attr(feature = "clap", clap(long, default_value = "0.0.0.0:8080"))]
    pub admin_addr: SocketAddr,
}

/// Supports configuring an admin server
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
pub struct Builder {
    addr: SocketAddr,
    ready: Readiness,

    #[cfg(feature = "metrics")]
    prometheus: metrics::PrometheusBuilder,
}

/// Supports spawning an admin server
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
pub struct Bound {
    addr: SocketAddr,
    ready: Readiness,
    server: hyper::server::Builder<hyper::server::conn::AddrIncoming>,

    #[cfg(feature = "metrics")]
    prometheus: metrics::PrometheusBuilder,
}

/// Controls how the admin server advertises readiness
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
#[derive(Clone, Debug)]
pub struct Readiness(Arc<AtomicBool>);

/// A handle to a running admin server
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
#[derive(Debug)]
pub struct Server {
    addr: SocketAddr,
    ready: Readiness,
    task: tokio::task::JoinHandle<Result<()>>,
}

// === impl AdminArgs ===

impl Default for AdminArgs {
    fn default() -> Self {
        Self {
            admin_addr: SocketAddr::from(([0, 0, 0, 0], 8080)),
        }
    }
}

impl AdminArgs {
    /// Creates a new [`Builder`] frm the command-line arguments
    pub fn into_builder(self) -> Builder {
        Builder::new(self.admin_addr)
    }
}

// === impl Builder ===

impl Builder {
    /// Creates a new [`Builder`] with the given server address
    ///
    /// The server starts unready by default and it's up to the caller to mark it as ready.
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            ready: Readiness(Arc::new(false.into())),

            #[cfg(feature = "metrics")]
            prometheus: Default::default(),
        }
    }

    /// Returns a readiness handle
    pub fn readiness(&self) -> Readiness {
        self.ready.clone()
    }

    /// Sets the initial readiness state to ready
    pub fn set_ready(&self) {
        self.ready.set(true);
    }

    /// Use the given `PrometheusBuilder` for the metrics endpoint.
    ///
    /// This method is only available if the "metrics" feature is enabled.
    ///
    /// **Note**: Builder methods that configure `metrics-exporter-prometheus`'s
    /// built-in HTTP listener, such as
    /// [`PrometheusBuilder::with_http_listener`][http] and
    /// [`PrometheusBuilder::add_allowed_address`][allowed] will not have an
    /// effect on the admin server's `/metrics` endpoint, since the HTTP
    /// server is managed by `kubert` rather than by `metrics-exporter-prometheus`.
    ///
    /// [http]: https://docs.rs/metrics-exporter-prometheus/latest/metrics_exporter_prometheus/struct.PrometheusBuilder.html#method.with_http_listener
    /// [allowed]: https://docs.rs/metrics-exporter-prometheus/latest/metrics_exporter_prometheus/struct.PrometheusBuilder.html#method.add_allowed_address
    // TODO(eliza): we may want to implement our own versions of these methods...
    #[cfg(feature = "metrics")]
    #[cfg_attr(docsrs, doc(cfg(feature = "metrics")))]
    pub fn set_prometheus(&mut self, prometheus: metrics::PrometheusBuilder) {
        self.prometheus = prometheus;
    }

    /// Binds the admin server without accepting connections
    pub fn bind(self) -> Result<Bound> {
        let Self {
            addr,
            ready,

            #[cfg(feature = "metrics")]
            prometheus,
        } = self;

        let server = hyper::server::Server::try_bind(&addr)?
            // Allow weird clients (like netcat).
            .http1_half_close(true)
            // Prevent port scanners, etc, from holding connections open.
            .http1_header_read_timeout(Duration::from_secs(2))
            // Use a small buffer, since we don't really transfer much data.
            .http1_max_buf_size(8 * 1024);

        Ok(Bound {
            addr,
            ready,
            server,

            #[cfg(feature = "metrics")]
            prometheus,
        })
    }
}

impl fmt::Debug for Builder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Builder");
        d.field("addr", &self.addr).field("ready", &self.ready);

        // The `PrometheusBuilder` type does not actually implement
        // `fmt::Debug`, but when the "metrics" feature is enabled, at least
        // indicate that it's there.
        #[cfg(feature = "metrics")]
        d.field("prometheus", &format_args!("PrometheusBuilder { ... }"));

        d.finish()
    }
}

// === impl Bound ===

impl Bound {
    /// Returns a readiness handle
    pub fn readiness(&self) -> Readiness {
        self.ready.clone()
    }

    /// Sets the initial readiness state to ready
    pub fn set_ready(&self) {
        self.ready.set(true);
    }

    /// Binds and runs the server on a background task, returning a handle
    pub fn spawn(self) -> Server {
        let ready = self.ready.clone();
        #[cfg(feature = "metrics")]
        let prometheus = metrics::Prometheus::new(self.prometheus);

        let server = {
            self.server
                .serve(hyper::service::make_service_fn(move |_conn| {
                    let ready = ready.clone();

                    #[cfg(feature = "metrics")]
                    let prometheus = prometheus.clone();

                    future::ok::<_, hyper::Error>(hyper::service::service_fn(
                        move |req: hyper::Request<hyper::Body>| match req.uri().path() {
                            "/live" => future::ok(handle_live(req)),
                            "/ready" => future::ok(handle_ready(&ready, req)),
                            #[cfg(feature = "metrics")]
                            "/metrics" => future::ok(prometheus.handle_metrics(req)),
                            _ => future::ok::<_, hyper::Error>(
                                Response::builder()
                                    .status(hyper::StatusCode::NOT_FOUND)
                                    .body(hyper::Body::default())
                                    .unwrap(),
                            ),
                        },
                    ))
                }))
        };

        let task = tokio::spawn(
            async move {
                debug!("Serving");
                server.await
            }
            .instrument(info_span!("admin", port = %self.addr.port())),
        );

        Server {
            task,
            addr: self.addr,
            ready: self.ready,
        }
    }
}

// === impl Readiness ===

impl Readiness {
    /// Gets the current readiness state
    pub fn get(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// Sets the readiness state
    pub fn set(&self, ready: bool) {
        self.0.store(ready, Ordering::Release);
    }
}

// === impl Server ===

impl Server {
    /// Returns the bound local address of the server
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Returns a readiness handle
    pub fn readiness(&self) -> Readiness {
        self.ready.clone()
    }

    /// Returns the server tasks's join handle
    pub fn into_join_handle(self) -> tokio::task::JoinHandle<Result<()>> {
        self.task
    }
}

// === handlers ===

fn handle_live(req: Request<Body>) -> Response<Body> {
    match *req.method() {
        hyper::Method::GET | hyper::Method::HEAD => Response::builder()
            .status(hyper::StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, "text/plain")
            .body("alive\n".into())
            .unwrap(),
        _ => Response::builder()
            .status(hyper::StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::default())
            .unwrap(),
    }
}

fn handle_ready(Readiness(ready): &Readiness, req: Request<Body>) -> Response<Body> {
    match *req.method() {
        hyper::Method::GET | hyper::Method::HEAD => {
            if ready.load(Ordering::Acquire) {
                return Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header(hyper::header::CONTENT_TYPE, "text/plain")
                    .body("ready\n".into())
                    .unwrap();
            }

            Response::builder()
                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                .header(hyper::header::CONTENT_TYPE, "text/plain")
                .body("not ready\n".into())
                .unwrap()
        }
        _ => Response::builder()
            .status(hyper::StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::default())
            .unwrap(),
    }
}
