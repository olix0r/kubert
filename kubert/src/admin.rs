//! Admin server utilities.
use ahash::AHashMap;
use futures_util::future;
use hyper::{Request, Response};
use std::{
    fmt,
    net::SocketAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{debug, info_span, Instrument};

/// An error binding an admin server.
#[derive(Debug, thiserror::Error)]
#[error("failed to bind admin server: {0}")]
pub struct BindError(#[from] std::io::Error);

type Body = http_body_util::Full<bytes::Bytes>;

/// A handler for a request path.
type RouteFn =
    Box<dyn Fn(Request<hyper::body::Incoming>) -> Response<Body> + Send + Sync + 'static>;

struct Route {
    call: RouteFn,
    require_loopback: bool,
}

#[cfg(feature = "prometheus-client")]
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
    routes: AHashMap<String, Route>,
}

/// Supports spawning an admin server
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
pub struct Bound {
    addr: SocketAddr,
    ready: Readiness,
    listener: tokio::net::TcpListener,
    server: hyper::server::conn::http1::Builder,
    routes: AHashMap<String, Route>,
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
    task: tokio::task::JoinHandle<Result<(), hyper::Error>>,
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

impl Default for Builder {
    fn default() -> Self {
        AdminArgs::default().into_builder()
    }
}

impl From<AdminArgs> for Builder {
    fn from(args: AdminArgs) -> Self {
        args.into_builder()
    }
}

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

    /// Use the provided prometheus Registry to export a `/metrics` endpoint
    /// on the admin server with process metrics. When the `tokio_unstable` cfg
    /// is set, tokio runtime metrics are also exported.
    ///
    /// This method is only available if the "prometheus-client" feature is enabled.
    #[cfg(feature = "prometheus-client")]
    #[cfg_attr(docsrs, doc(cfg(feature = "prometheus-client")))]
    pub fn with_prometheus(self, mut registry: prometheus_client::registry::Registry) -> Self {
        #[cfg(not(tokio_unstable))]
        tracing::debug!("Tokio runtime metrics cannot be monitored without the tokio_unstable cfg");
        #[cfg(tokio_unstable)]
        {
            let metrics = kubert_prometheus_tokio::Runtime::register(
                registry.sub_registry_with_prefix("tokio_rt"),
                tokio::runtime::Handle::current(),
            );
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            tokio::spawn(
                async move { metrics.updated(&mut interval).await }
                    .instrument(tracing::info_span!("kubert-prom-tokio-rt")),
            );
        }

        if let Err(error) =
            kubert_prometheus_process::register(registry.sub_registry_with_prefix("process"))
        {
            tracing::warn!(%error, "Process metrics cannot be monitored");
        }

        self.with_prometheus_handler("/metrics", registry)
    }

    /// Use the provided prometheus Registry to export an arbitrary metrics
    /// endpoint.
    ///
    /// This method is only available if the "prometheus-client" feature is enabled.
    #[cfg(feature = "prometheus-client")]
    #[cfg_attr(docsrs, doc(cfg(feature = "prometheus-client")))]
    pub fn with_prometheus_handler(
        self,
        path: impl ToString,
        registry: prometheus_client::registry::Registry,
    ) -> Self {
        let prom = metrics::Prometheus::new(registry);
        self.with_handler(path, move |req| prom.handle_metrics(req))
    }

    #[cfg(feature = "runtime-diagnostics")]
    pub(crate) fn with_runtime_diagnostics(self, diagnostics: crate::runtime::Diagnostics) -> Self {
        self.with_loopback_handler("/kubert.json", move |req| {
            let with_resources = req.uri().query() == Some("resources");
            let summary = diagnostics.summarize(with_resources);

            let mut bytes = Vec::with_capacity(8 * 1024);
            serde_json::to_writer_pretty(&mut bytes, &summary)
                .expect("json serialization must succeed");
            hyper::Response::builder()
                .header(hyper::header::CONTENT_TYPE, "application/json")
                .body(Body::from(bytes))
                .expect("response must succeed")
        })
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
    pub fn with_handler(
        mut self,
        path: impl ToString,
        handler: impl Fn(Request<hyper::body::Incoming>) -> Response<Body> + Send + Sync + 'static,
    ) -> Self {
        let path = path.to_string();
        let route = Self::new_route(&path, handler);
        self.routes.insert(path, route);
        self
    }
    /// Adds a request handler for `path` to the admin server that only accepts
    /// requests from the loopback interface.
    ///
    /// Requests to `path` will be handled by invoking the provided `handler`
    /// function with each request. This can be used to add additional
    /// functionality to the admin server. The handler will only be invoked for
    /// requests that originate from the loopback interface. All other requests
    /// will receive a `403 Forbidden` response.
    ///
    /// # Panics
    ///
    /// This method panics if called with the path `/ready` or `/live`, as these
    /// paths would conflict with the built-in readiness and liveness endpoints.
    pub fn with_loopback_handler(
        mut self,
        path: impl ToString,
        handler: impl Fn(Request<hyper::body::Incoming>) -> Response<Body> + Send + Sync + 'static,
    ) -> Self {
        let path = path.to_string();
        let mut route = Self::new_route(&path, handler);
        route.require_loopback = true;
        self.routes.insert(path, route);
        self
    }

    fn new_route(
        path: &str,
        handler: impl Fn(Request<hyper::body::Incoming>) -> Response<Body> + Send + Sync + 'static,
    ) -> Route {
        let path = path.to_string();
        assert_ne!(
            path, "/ready",
            "the built-in `/ready` handler cannot be overridden"
        );
        assert_ne!(
            path, "/live",
            "the built-in `/live` handler cannot be overridden"
        );
        Route {
            call: Box::new(handler),
            require_loopback: false,
        }
    }

    /// Binds the admin server without accepting connections
    pub fn bind(self) -> Result<Bound, BindError> {
        let Self {
            addr,
            ready,
            routes,
        } = self;

        let lis = std::net::TcpListener::bind(addr)?;
        lis.set_nonblocking(true)?;
        let listener = tokio::net::TcpListener::from_std(lis)?;

        let mut server = hyper::server::conn::http1::Builder::new();
        server
            // Allow weird clients (like netcat).
            .half_close(true)
            .timer(hyper_util::rt::TokioTimer::default())
            // Prevent port scanners, etc, from holding connections open.
            .header_read_timeout(Duration::from_secs(2))
            // Use a small buffer, since we don't really transfer much data.
            .max_buf_size(8 * 1024);

        Ok(Bound {
            addr,
            ready,
            server,
            listener,
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
        let Self {
            ready,
            server,
            listener,
            routes,
            addr,
        } = self;

        let task = tokio::spawn({
            let ready = ready.clone();
            let routes = Arc::new(routes);
            async move {
                loop {
                    let (stream, client_addr) = match listener.accept().await {
                        Ok(socket) => socket,
                        Err(error) => {
                            tracing::warn!(%error, "Failed to accept connection");
                            continue;
                        }
                    };
                    if let Err(error) = stream.set_nodelay(true) {
                        tracing::warn!(%error, "Failed to set TCP_NODELAY");
                    }
                    tracing::trace!(client.addr = ?client_addr, "Accepted connection");

                    let svc = {
                        use tower::ServiceExt;
                        let ready = ready.clone();
                        let routes = routes.clone();
                        let svc =
                            tower::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                handle(client_addr, &ready, &routes, req)
                            });
                        #[cfg(any(feature = "admin-brotli", feature = "admin-gzip"))]
                        let svc = tower_http::compression::Compression::new(svc);
                        hyper::service::service_fn(move |req| svc.clone().oneshot(req))
                    };

                    let serve =
                        server.serve_connection(hyper_util::rt::TokioIo::new(stream), svc.clone());
                    tokio::spawn(
                        async move {
                            debug!("Serving");
                            serve.await
                        }
                        .instrument(
                            tracing::debug_span!("conn", client.addr = %client_addr).or_current(),
                        ),
                    );
                }
            }
            .instrument(info_span!("admin", port = %self.addr.port()))
        });

        Server { task, addr, ready }
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
    pub fn into_join_handle(self) -> tokio::task::JoinHandle<Result<(), hyper::Error>> {
        self.task
    }
}

// === routes ===

fn handle(
    client_addr: SocketAddr,
    ready: &Readiness,
    routes: &Arc<AHashMap<String, Route>>,
    req: hyper::Request<hyper::body::Incoming>,
) -> Pin<
    Box<
        dyn std::future::Future<Output = Result<hyper::Response<Body>, tokio::task::JoinError>>
            + Send,
    >,
> {
    // Fast path for probe handlers.
    if req.uri().path() == "/live" {
        Box::pin(future::ok(handle_live(req)))
    } else if req.uri().path() == "/ready" {
        Box::pin(future::ok(handle_ready(ready, req)))
    } else if routes.contains_key(req.uri().path()) {
        // User-provided handlers--especially metrics collectors--may perform
        // blocking calls like stat. Prevent these tasks from blocking the
        // runtime.
        let routes = routes.clone();
        let path = req.uri().path().to_string();
        let is_loopback = client_addr.ip().is_loopback();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let route = routes.get(&path).expect("routes must contain path");
                if route.require_loopback && !is_loopback {
                    return Response::builder()
                        .status(hyper::StatusCode::FORBIDDEN)
                        .body(Body::default())
                        .unwrap();
                }
                (route.call)(req)
            })
            .await
        })
    } else {
        Box::pin(future::ok(
            Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .body(Body::default())
                .unwrap(),
        ))
    }
}

fn handle_live(req: Request<hyper::body::Incoming>) -> Response<Body> {
    match *req.method() {
        hyper::Method::GET | hyper::Method::HEAD => Response::builder()
            .status(hyper::StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, "text/plain")
            .body("alive\n".into())
            .unwrap(),
        _ => Response::builder()
            .status(hyper::StatusCode::METHOD_NOT_ALLOWED)
            .header(hyper::header::ALLOW, "GET, HEAD")
            .body(Body::default())
            .unwrap(),
    }
}

fn handle_ready(
    Readiness(ready): &Readiness,
    req: Request<hyper::body::Incoming>,
) -> Response<Body> {
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
            .header(hyper::header::ALLOW, "GET, HEAD")
            .body(Body::default())
            .unwrap(),
    }
}
