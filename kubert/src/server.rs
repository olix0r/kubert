//! Helpers for configuring and running an HTTPS server, especially for admission controllers and
//! API extensions
//!
//! Unlike a normal `hyper` server, this server reloads its TLS credentials for each connection to
//! support certificate rotation.

use std::net::SocketAddr;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tower::Service;
use tracing::{debug, error, info, info_span, Instrument};

#[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
mod tls;
#[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
use tls::{TlsCertPath, TlsKeyPath};

/// Command-line arguments used to configure a server
#[derive(Clone, Debug)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub struct ServerArgs {
    /// The server's address
    #[cfg_attr(feature = "clap", clap(long, default_value = "0.0.0.0:443"))]
    pub server_addr: SocketAddr,

    /// The path to the server's TLS key
    #[cfg_attr(feature = "clap", clap(long))]
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
    #[cfg_attr(
        docsrs,
        doc(cfg(all(
            feature = "server",
            any(feature = "rustls-tls", feature = "boring-tls")
        )))
    )]
    pub server_tls_key: Option<TlsKeyPath>,

    /// The path to the server's TLS certificate
    #[cfg_attr(feature = "clap", clap(long))]
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
    #[cfg_attr(
        docsrs,
        doc(cfg(all(
            feature = "server",
            any(feature = "rustls-tls", feature = "boring-tls")
        )))
    )]
    pub server_tls_certs: Option<TlsCertPath>,
}

/// A running server
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub struct Bound {
    local_addr: SocketAddr,
    tcp: tokio::net::TcpListener,

    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
    tls_key: TlsKeyPath,
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
    tls_certs: TlsCertPath,
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
    #[cfg_attr(
        docsrs,
        doc(cfg(all(
            feature = "server",
            any(feature = "rustls-tls", feature = "boring-tls")
        )))
    )]
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
    NoTlsKey,

    /// No TLS certificate path was configured
    #[error("--server-tls-certs must be set")]
    #[cfg_attr(
        docsrs,
        doc(cfg(all(
            feature = "server",
            any(feature = "rustls-tls", feature = "boring-tls")
        )))
    )]
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
    NoTlsCerts,

    /// The configured TLS certificate path could not be read
    #[error("failed to read TLS certificates: {0}")]
    #[cfg_attr(
        docsrs,
        doc(cfg(all(
            feature = "server",
            any(feature = "rustls-tls", feature = "boring-tls")
        )))
    )]
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
    TlsCertsReadError(#[source] std::io::Error),

    /// The configured TLS key path could not be read
    #[error("failed to read TLS key: {0}")]
    #[cfg_attr(
        docsrs,
        doc(cfg(all(
            feature = "server",
            any(feature = "rustls-tls", feature = "boring-tls")
        )))
    )]
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
    TlsKeyReadError(#[source] std::io::Error),

    /// The configured TLS credentials were invalid
    #[error("failed to load TLS credentials: {0}")]
    #[cfg_attr(
        docsrs,
        doc(cfg(all(
            feature = "server",
            any(feature = "rustls-tls", feature = "boring-tls")
        )))
    )]
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
    InvalidTlsCredentials(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// An error occurred while binding a server
    #[error("failed to bind {0:?}: {1}")]
    Bind(SocketAddr, #[source] std::io::Error),

    /// An error occurred while reading a bound server's local address
    #[error("failed to get bound local address: {0}")]
    LocalAddr(#[source] std::io::Error),
}

// === impl ServerArgs ===

impl ServerArgs {
    /// Attempts to load credentials and bind the server socket
    pub async fn bind(self) -> Result<Bound, Error> {
        #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
        let tls_key = self.server_tls_key.ok_or(Error::NoTlsKey)?;
        #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
        let tls_certs = self.server_tls_certs.ok_or(Error::NoTlsCerts)?;

        // Ensure the TLS key and certificate files load properly before binding the socket and
        // spawning the server.
        #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
        let _ = tls::load_tls(&tls_key, &tls_certs).await?;

        let tcp = TcpListener::bind(&self.server_addr)
            .await
            .map_err(|e| Error::Bind(self.server_addr, e))?;
        let local_addr = tcp.local_addr().map_err(Error::LocalAddr)?;
        Ok(Bound {
            local_addr,
            tcp,
            #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
            tls_key,
            #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
            tls_certs,
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
        S: Service<hyper::Request<hyper::Body>, Response = hyper::Response<B>>
            + Clone
            + Send
            + 'static,
        S::Error: std::error::Error + Send + Sync,
        S::Future: Send,
        B: hyper::body::HttpBody + Send + 'static,
        B::Data: Send,
        B::Error: std::error::Error + Send + Sync,
    {
        let Self {
            local_addr,
            tcp,
            tls_key,
            tls_certs,
        } = self;

        let task = tokio::spawn(
            accept_loop(
                tcp,
                drain,
                service,
                #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
                tls_key,
                #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
                tls_certs,
            )
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

async fn accept_loop<S, B>(
    tcp: TcpListener,
    drain: drain::Watch,
    service: S,
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))] tls_key: TlsKeyPath,
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))] tls_certs: TlsCertPath,
) where
    S: Service<hyper::Request<hyper::Body>, Response = hyper::Response<B>> + Clone + Send + 'static,
    S::Error: std::error::Error + Send + Sync,
    S::Future: Send,
    B: hyper::body::HttpBody + Send + 'static,
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

        let client_addr = match socket.peer_addr() {
            Ok(addr) => addr,
            Err(error) => {
                error!(%error, "Failed to get peer address");
                continue;
            }
        };

        tokio::spawn(
            serve_conn(
                socket,
                drain.clone(),
                service.clone(),
                #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
                tls_key.clone(),
                #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
                tls_certs.clone(),
            )
            .instrument(info_span!(
                "conn",
                client.ip = %client_addr.ip(),
                client.port = %client_addr.port(),
            )),
        );
    }
}

async fn serve_conn<S, B>(
    socket: TcpStream,
    drain: drain::Watch,
    service: S,
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))] tls_key: TlsKeyPath,
    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))] tls_certs: TlsCertPath,
) where
    S: Service<hyper::Request<hyper::Body>, Response = hyper::Response<B>> + Send + 'static,
    S::Error: std::error::Error + Send + Sync,
    S::Future: Send,
    B: hyper::body::HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: std::error::Error + Send + Sync,
{
    tracing::debug!("accepted TCP connection");

    #[cfg(any(feature = "rustls-tls", feature = "boring-tls"))]
    let socket = {
        // Reload the TLS credentials for each connection.
        let tls = match tls::load_tls(&tls_key, &tls_certs).await {
            Ok(tls) => tls,
            Err(error) => {
                info!(%error, "Connection failed");
                return;
            }
        };
        tracing::trace!("loaded TLS credentials");

        let socket = match tls::accept(&tls, socket).await {
            Ok(s) => s,
            Err(error) => {
                info!(%error, "TLS handshake failed");
                return;
            }
        };
        tracing::trace!("TLS handshake completed");
        socket
    };

    // Serve the HTTP connection and wait for the drain signal. If a drain is
    // signaled, tell the HTTP connection to terminate gracefully when in-flight
    // requests have completed.
    let mut conn = hyper::server::conn::Http::new()
        // Prevent port scanners, etc, from holding connections open.
        .http1_header_read_timeout(std::time::Duration::from_secs(2))
        .serve_connection(socket, service);
    let res = tokio::select! {
        biased;
        res = &mut conn => res,
        release = drain.signaled() => {
            Pin::new(&mut conn).graceful_shutdown();
            release.release_after(conn).await
        }
    };
    match res {
        Ok(()) => debug!("Connection closed"),
        Err(error) => info!(%error, "Connection lost"),
    }
}
