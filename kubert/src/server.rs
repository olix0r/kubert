//! Helpers for configuring and running an HTTPS server, especially for admission controllers and
//! API extensions
//!
//! Unlike a normal `hyper` server, this server reloads its TLS credentials for each connection to
//! support certificate rotation.
//!
//! # TLS Feature Flags
//!
//! The server module requires that one of the [TLS implementation Cargo
//! features](crate#tls-features) be enabled in order to run the server.
//! If neither TLS implementation is selected, running the server will panic.
//! However, this module itself is still enabled if neither TLS feature flag is
//! selected. This is to allow the server module to be used in a library crate
//! which does not require either particular TLS implementation, so that the
//! top-level binary crate may choose which TLS implementation is used.

#![cfg_attr(
    not(any(feature = "rustls-tls", feature = "openssl-tls")),
    allow(dead_code, unused_variables)
)]

use std::{convert::Infallible, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tower::Service;
use tracing::{debug, error, info, info_span, Instrument};

#[cfg(feature = "rustls-tls")]
mod tls_rustls;

#[cfg(feature = "openssl-tls")]
mod tls_openssl;

#[cfg(test)]
mod tests;

/// Command-line arguments used to configure a server
#[derive(Clone, Debug)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub struct ServerArgs {
    /// The server's address
    #[cfg_attr(feature = "clap", clap(long, default_value = "0.0.0.0:443"))]
    pub server_addr: SocketAddr,

    /// The path to the server's TLS key file.
    ///
    /// This should be a PEM-encoded file containing a single PKCS#8 or RSA
    /// private key.
    #[cfg_attr(feature = "clap", clap(long))]
    pub server_tls_key: Option<TlsKeyPath>,

    /// The path to the server's TLS certificate file.
    ///
    /// This should be a PEM-encoded file containing at least one TLS end-entity
    /// certificate.
    #[cfg_attr(feature = "clap", clap(long))]
    pub server_tls_certs: Option<TlsCertPath>,
}

/// A running server
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub struct Bound {
    local_addr: SocketAddr,
    tcp: tokio::net::TcpListener,
    tls: Arc<TlsPaths>,
}

/// A running server
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub struct SpawnedServer {
    local_addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}
/// Describes an error that occurred while initializing a server
#[derive(Debug, Error)]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
#[non_exhaustive]
pub enum Error {
    /// No TLS key path was configured
    #[error("--server-tls-key must be set")]
    NoTlsKey,

    /// No TLS certificate path was configured
    #[error("--server-tls-certs must be set")]
    NoTlsCerts,

    /// The configured TLS certificate path could not be read
    #[error("failed to read TLS certificates: {0}")]
    TlsCertsReadError(#[source] std::io::Error),

    /// The configured TLS key path could not be read
    #[error("failed to read TLS key: {0}")]
    TlsKeyReadError(#[source] std::io::Error),

    /// The configured TLS credentials were invalid
    #[error("failed to load TLS credentials: {0}")]
    InvalidTlsCredentials(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// An error occurred while binding a server
    #[error("failed to bind {0:?}: {1}")]
    Bind(SocketAddr, #[source] std::io::Error),

    /// An error occurred while reading a bound server's local address
    #[error("failed to get bound local address: {0}")]
    LocalAddr(#[source] std::io::Error),
}

/// The path to the server's TLS private key
#[derive(Clone, Debug)]
pub struct TlsKeyPath(PathBuf);

/// The path to the server's TLS certificate bundle
#[derive(Clone, Debug)]
pub struct TlsCertPath(PathBuf);

#[derive(Clone, Debug)]
// TLS paths may not be used if TLS is not enabled.
struct TlsPaths {
    key: TlsKeyPath,
    certs: TlsCertPath,
}

// === impl ServerArgs ===

impl ServerArgs {
    /// Attempts to load credentials and bind the server socket
    ///
    /// # Panics
    ///
    /// This method panics if neither of [the "rustls-tls" or "openssl-tls" Cargo
    /// features][tls-features] are enabled. See [the module-level
    /// documentation][tls-doc] for details.
    ///
    /// [tls-features]: crate#tls-features
    /// [tls-doc]: crate::server#tls-feature-flags
    pub async fn bind(self) -> Result<Bound, Error> {
        let tls = {
            let key = self.server_tls_key.ok_or(Error::NoTlsKey)?;
            let certs = self.server_tls_certs.ok_or(Error::NoTlsCerts)?;
            // Ensure the TLS key and certificate files load properly before binding the socket and
            // spawning the server.

            #[cfg(all(not(feature = "rustls-tls"), feature = "openssl-tls"))]
            let _ = tls_openssl::load_tls(&key, &certs).await?;
            #[cfg(feature = "rustls-tls")]
            let _ = tls_rustls::load_tls(&key, &certs).await?;

            Arc::new(TlsPaths { key, certs })
        };

        let tcp = TcpListener::bind(&self.server_addr)
            .await
            .map_err(|e| Error::Bind(self.server_addr, e))?;
        let local_addr = tcp.local_addr().map_err(Error::LocalAddr)?;
        Ok(Bound {
            local_addr,
            tcp,
            tls,
        })
    }
}

impl Bound {
    /// Returns the bound local address of the server
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Bind an HTTPS server to the configured address with the provided service
    ///
    /// The server terminates gracefully when the provided `drain` handle is signaled.
    ///
    /// TLS credentials are read from the configured paths _for each connection_ to support
    /// certificate rotation. As such, it is not recommended to expose this server to the open
    /// internet or to clients that open many short-lived connections. It is primarily intended for
    /// kubernetes admission controllers.
    pub fn spawn<S, B>(self, service: S, drain: drain::Watch) -> SpawnedServer
    where
        S: Service<hyper::Request<hyper::body::Incoming>, Response = hyper::Response<B>>
            + Clone
            + Send
            + 'static,
        S::Error: std::error::Error + Send + Sync,
        S::Future: Send,
        B: hyper::body::Body + Send + 'static,
        B::Data: Send,
        B::Error: std::error::Error + Send + Sync,
    {
        let Self {
            local_addr,
            tcp,
            tls,
        } = self;

        let task = tokio::spawn(
            accept_loop(tcp, drain, service, tls)
                .instrument(info_span!("server", port = %local_addr.port())),
        );

        SpawnedServer { local_addr, task }
    }
}

// === impl SpawnedServer ===

impl SpawnedServer {
    /// Returns the bound local address of the spawned server
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Terminates the server task forcefully
    pub fn abort(&self) {
        self.task.abort();
    }

    /// Waits for the server task to complete
    pub async fn join(self) -> Result<(), tokio::task::JoinError> {
        self.task.await
    }
}

async fn accept_loop<S, B>(tcp: TcpListener, drain: drain::Watch, service: S, tls: Arc<TlsPaths>)
where
    S: Service<hyper::Request<hyper::body::Incoming>, Response = hyper::Response<B>>
        + Clone
        + Send
        + 'static,
    S::Error: std::error::Error + Send + Sync,
    S::Future: Send,
    B: hyper::body::Body + Send + 'static,
    B::Data: Send,
    B::Error: std::error::Error + Send + Sync,
{
    tracing::debug!("listening");
    loop {
        tracing::trace!("accepting");
        // Wait for the shutdown to be signaled or for the next connection to be accepted.
        let socket = tokio::select! {
            biased;

            release = drain.clone().signaled() => {
                drop(release);
                return;
            }

            res = tcp.accept() => match res {
                Ok((socket, _)) => socket,
                Err(error) => {
                    error!(%error, "Failed to accept connection");
                    continue;
                }
            },
        };

        if let Err(error) = socket.set_nodelay(true) {
            error!(%error, "Failed to set TCP_NODELAY");
            continue;
        }

        let client_addr = match socket.peer_addr() {
            Ok(addr) => addr,
            Err(error) => {
                error!(%error, "Failed to get peer address");
                continue;
            }
        };

        tokio::spawn(
            serve_conn(socket, drain.clone(), service.clone(), tls.clone()).instrument(info_span!(
                "conn",
                client.ip = %client_addr.ip(),
                client.port = %client_addr.port(),
            )),
        );
    }
}

async fn serve_conn<S, B>(socket: TcpStream, drain: drain::Watch, service: S, tls: Arc<TlsPaths>)
where
    S: Service<hyper::Request<hyper::body::Incoming>, Response = hyper::Response<B>>
        + Clone
        + Send
        + 'static,
    S::Error: std::error::Error + Send + Sync,
    S::Future: Send,
    B: hyper::body::Body + Send + 'static,
    B::Data: Send,
    B::Error: std::error::Error + Send + Sync,
{
    tracing::debug!("accepted TCP connection");

    let socket = {
        let TlsPaths { ref key, ref certs } = &*tls;
        // Reload the TLS credentials for each connection.

        #[cfg(all(not(feature = "rustls-tls"), feature = "openssl-tls"))]
        let res = tls_openssl::load_tls(key, certs).await;
        #[cfg(feature = "rustls-tls")]
        let res = tls_rustls::load_tls(key, certs).await;
        #[cfg(not(any(feature = "rustls-tls", feature = "openssl-tls")))]
        let res = {
            enum Accept {}
            Err::<Accept, _>(std::io::Error::new(
                std::io::ErrorKind::Other,
                "TLS support not enabled",
            ))
        };
        let tls = match res {
            Ok(tls) => tls,
            Err(error) => {
                info!(%error, "Connection failed");
                return;
            }
        };
        tracing::trace!("loaded TLS credentials");

        #[cfg(all(not(feature = "rustls-tls"), feature = "openssl-tls"))]
        let res = tls_openssl::accept(&tls, socket).await;
        #[cfg(feature = "rustls-tls")]
        let res = tls_rustls::accept(&tls, socket).await;
        #[cfg(not(any(feature = "rustls-tls", feature = "openssl-tls")))]
        let res = Err::<TcpStream, _>(std::io::Error::new(
            std::io::ErrorKind::Other,
            "TLS support not enabled",
        ));
        let socket = match res {
            Ok(s) => s,
            Err(error) => {
                info!(%error, "TLS handshake failed");
                return;
            }
        };
        tracing::trace!("TLS handshake completed");

        socket
    };

    #[derive(Copy, Clone, Debug)]
    struct Executor;
    impl<F> hyper::rt::Executor<F> for Executor
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        fn execute(&self, fut: F) {
            tokio::spawn(fut.in_current_span());
        }
    }

    #[cfg(any(feature = "server-brotli", feature = "server-gzip"))]
    let service = tower_http::decompression::Decompression::new(
        tower_http::compression::Compression::new(service),
    );

    // Serve the HTTP connection and wait for the drain signal. If a drain is
    // signaled, tell the HTTP connection to terminate gracefully when in-flight
    // requests have completed.
    let mut builder = hyper_util::server::conn::auto::Builder::new(Executor);
    // Prevent port scanners, etc, from holding connections open.
    builder
        .http1()
        .header_read_timeout(std::time::Duration::from_secs(2))
        .timer(hyper_util::rt::TokioTimer::default());
    let graceful = hyper_util::server::graceful::GracefulShutdown::new();
    let conn = graceful.watch(
        builder
            .serve_connection(
                hyper_util::rt::TokioIo::new(socket),
                hyper_util::service::TowerToHyperService::new(service),
            )
            .into_owned(),
    );
    tokio::spawn(
        async move {
            match conn.await {
                Ok(()) => debug!("Connection closed"),
                Err(error) => info!(%error, "Connection lost"),
            }
        }
        .in_current_span(),
    );

    let latch = drain.signaled().await;
    latch.release_after(graceful.shutdown()).await;
}

// === impl TlsCertPath ===

impl FromStr for TlsCertPath {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

// === impl TlsKeyPath ===

impl FromStr for TlsKeyPath {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}
