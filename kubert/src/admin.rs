//! Admin server utilities.
use futures_util::future;
use hyper::{Body, Request, Response};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{debug, info_span, Instrument};

/// Command-line arguments used to configure an admin server
#[cfg(feature = "clap")]
#[derive(Clone, Debug, clap::Parser)]
#[cfg_attr(docsrs, doc(cfg(all(feature = "admin", feature = "clap"))))]
pub struct AdminArgs {
    /// The admin server's address
    #[cfg_attr(feature = "clap", clap(long, default_value = "0.0.0.0:8080"))]
    pub admin_addr: SocketAddr,
}

/// Supports configuring and running an admin server
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
#[derive(Debug)]
pub struct Builder {
    addr: SocketAddr,
    ready: Readiness,
}

/// Controls how the admin server advertises readiness
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
#[derive(Clone, Debug)]
pub struct Readiness(Arc<AtomicBool>);

/// A handle to a running admin server
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
pub struct Server {
    addr: SocketAddr,
    ready: Readiness,
    task: tokio::task::JoinHandle<hyper::Result<()>>,
}

// === impl AdminArgs ===

#[cfg(feature = "clap")]
impl AdminArgs {
    /// Creates a new [`Builder`] frm the command-line arguments
    pub fn into_builder(self) -> Builder {
        Builder::new(self.admin_addr)
    }

    /// Binds and runs the server on a background task, returning a handle
    ///
    /// The server starts unready by default and it's up to the caller to mark it as ready.
    pub fn spawn(self) -> Server {
        self.into_builder().spawn()
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

    /// Binds and runs the server on a background task, returning a handle
    pub fn spawn(self) -> Server {
        let Self { addr, ready } = self;

        let http = hyper::server::Server::bind(&addr)
            // Allow weird clients (like netcat).
            .http1_half_close(true)
            // Prevent port scanners, etc, from holding connections ope.n
            .http1_header_read_timeout(Duration::from_secs(2))
            // Use a small buffer, since we don't really transfer much data.
            .http1_max_buf_size(8 * 1024);

        let server = {
            let ready = ready.clone();
            http.serve(hyper::service::make_service_fn(move |_conn| {
                let ready = ready.clone();
                future::ok::<_, hyper::Error>(hyper::service::service_fn(
                    move |req: hyper::Request<hyper::Body>| match req.uri().path() {
                        "/live" => future::ok(handle_live(req)),
                        "/ready" => future::ok(handle_ready(&ready, req)),
                        _ => future::ok::<_, hyper::Error>(
                            hyper::Response::builder()
                                .status(hyper::StatusCode::NOT_FOUND)
                                .body(hyper::Body::default())
                                .unwrap(),
                        ),
                    },
                ))
            }))
        };

        let addr = server.local_addr();

        let task = tokio::spawn(
            async move {
                debug!("Serving");
                server.await
            }
            .instrument(info_span!("admin", port = %addr.port())),
        );

        Server { addr, ready, task }
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
    pub fn into_join_handle(self) -> tokio::task::JoinHandle<hyper::Result<()>> {
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
