//! Admin server utilities.
use ahash::AHashMap;
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

/// A handler for a request path.
type HandlerFn = Box<dyn Fn(Request<Body>) -> Response<Body> + Send + Sync + 'static>;

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
    routes: AHashMap<String, HandlerFn>,
}

/// Supports spawning an admin server
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
pub struct Bound {
    addr: SocketAddr,
    ready: Readiness,
    server: hyper::server::Builder<hyper::server::conn::AddrIncoming>,
    routes: AHashMap<String, HandlerFn>,
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
            routes: Default::default(),
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

    /// Use the default `PrometheusBuilder` to configure a `/metrics` endpoint
    /// on the admin server. Process metrics are exposed by default.
    ///
    /// This method is only available if the "metrics" feature is enabled.
    #[cfg(feature = "metrics")]
    #[cfg_attr(docsrs, doc(cfg(feature = "metrics")))]
    pub fn with_default_prometheus(&mut self) -> &mut Self {
        let metrics = metrics::PrometheusBuilder::new()
            .install_recorder()
            .expect("failed to install Prometheus recorder");

        let process = metrics_process::Collector::default();
        process.describe();

        self.add_prometheus_handler("/metrics", metrics, move || process.collect())
    }

    /// Use the given `PrometheusHandle` to add a metrics route to the admin
    /// server.
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
    #[cfg(feature = "metrics")]
    #[cfg_attr(docsrs, doc(cfg(feature = "metrics")))]
    pub fn add_prometheus_handler(
        &mut self,
        path: impl ToString,
        metrics: metrics::PrometheusHandle,
        collect: impl Fn() + Send + Sync + 'static,
    ) -> &mut Self {
        let prom = metrics::Prometheus::new(metrics, collect);
        self.add_handler(path, move |req| prom.handle_metrics(req))
    }

    /// Adds a request handler for `path` to the admin server.
    ///
    /// Requests to `path` will be handled by invoking the provided `handler`
    /// function with each request. This can be used to add additional
    /// functionality to the admin server.
    ///
    /// # Panics
    ///
    /// This method panics if called with the path `/ready` or `/live`, as these
    /// paths would conflict with the built-in readiness and liveness endpoints.
    pub fn add_handler(
        &mut self,
        path: impl ToString,
        handler: impl Fn(Request<Body>) -> Response<Body> + Send + Sync + 'static,
    ) -> &mut Self {
        let path = path.to_string();
        assert_ne!(
            path, "/ready",
            "the built-in `/ready` handler cannot be overridden"
        );
        assert_ne!(
            path, "/live",
            "the built-in `/live` handler cannot be overridden"
        );
        self.routes.insert(path, Box::new(handler));
        self
    }

    /// Binds the admin server without accepting connections
    pub fn bind(self) -> Result<Bound> {
        let Self {
            addr,
            ready,
            routes,
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
            routes,
        })
    }
}

impl fmt::Debug for Builder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Builder");
        d.field("addr", &self.addr).field("ready", &self.ready);

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
        let routes = Arc::new(self.routes);

        let server = {
            self.server
                .serve(hyper::service::make_service_fn(move |_conn| {
                    let ready = ready.clone();
                    let routes = routes.clone();

                    future::ok::<_, hyper::Error>(hyper::service::service_fn(
                        move |req: hyper::Request<hyper::Body>| {
                            future::ok::<_, hyper::Error>(match req.uri().path() {
                                "/live" => handle_live(req),
                                "/ready" => handle_ready(&ready, req),
                                path => routes
                                    .get(path)
                                    .map(|handler| handler(req))
                                    .unwrap_or_else(|| {
                                        Response::builder()
                                            .status(hyper::StatusCode::NOT_FOUND)
                                            .body(hyper::Body::default())
                                            .unwrap()
                                    }),
                            })
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

// === routes ===

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
