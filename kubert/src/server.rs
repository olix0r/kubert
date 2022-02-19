//! Helpers for configuring and running an HTTPS server, especially for admission controllers and
//! API extensions.

use std::{convert::Infallible, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{rustls, TlsAcceptor};
use tower_service::Service;
use tracing::{debug, error, info, info_span, Instrument};

/// Command-line arguments used to configure a server.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct ServerArgs {
    #[cfg_attr(feature = "clap", clap(long, default_value = "0.0.0.0:443"))]
    pub server_addr: SocketAddr,

    #[cfg_attr(feature = "clap", clap(long))]
    pub server_tls_key: Option<TlsKeyPath>,

    #[cfg_attr(feature = "clap", clap(long))]
    pub server_tls_certs: Option<TlsCertPath>,
}

/// A running server.
pub struct SpawnedServer {
    local_addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

/// The path to the server's TLS private key.
#[derive(Clone, Debug)]
pub struct TlsKeyPath(PathBuf);

/// The path to the server's TLS certificate bundle.
#[derive(Clone, Debug)]
pub struct TlsCertPath(PathBuf);

#[derive(Debug, Error)]
pub enum Error {
    #[error("--server-tls-key must be set")]
    NoTlsKey,

    #[error("--server-tls-certs must be set")]
    NoTlsCerts,

    #[error("failed to read TLS certificates: {0}")]
    TlsCertsReadError(#[source] std::io::Error),

    #[error("failed to read TLS key: {0}")]
    TlsKeyReadError(#[source] std::io::Error),

    #[error("failed to load tls credentials: {0}")]
    InvalidTlsCredentials(#[source] rustls::Error),

    #[error("failed to bind {0:?}: {1}")]
    Bind(SocketAddr, #[source] std::io::Error),

    #[error("failed to get bound local address: {0}")]
    LocalAddr(#[source] std::io::Error),
}

impl ServerArgs {
    /// Bind an HTTPS server to the configured address with the provided service.
    ///
    /// The server terminates gracefully when the provided `drain` handle is signaled.
    pub async fn spawn<S, B>(self, service: S, drain: drain::Watch) -> Result<SpawnedServer, Error>
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
        let tls_key = self.server_tls_key.ok_or(Error::NoTlsKey)?;
        let tls_certs = self.server_tls_certs.ok_or(Error::NoTlsCerts)?;

        // Ensure the TLS key and certificate files load properly before binding the socket and
        // spawning the server.
        let _ = load_tls(&tls_key, &tls_certs).await?;

        let tcp = TcpListener::bind(&self.server_addr)
            .await
            .map_err(|e| Error::Bind(self.server_addr, e))?;
        let local_addr = tcp.local_addr().map_err(Error::LocalAddr)?;

        let task = tokio::spawn(
            accept_loop(tcp, drain, service, tls_key, tls_certs)
                .instrument(info_span!("server", port = %local_addr.port())),
        );

        Ok(SpawnedServer { local_addr, task })
    }
}

// === impl SpawnedServer ===

impl SpawnedServer {
    /// Returns the bound local address of the spawned server.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Terminates the server task forcefully.
    pub fn abort(&self) {
        self.task.abort();
    }

    /// Waits for the server task to complete.
    pub async fn join(self) -> Result<(), tokio::task::JoinError> {
        self.task.await
    }
}

// === impl TlsCertPath ===

impl TlsCertPath {
    // Load public certificate from file.
    async fn load_certs(&self) -> std::io::Result<Vec<rustls::Certificate>> {
        // Open certificate file.
        let pem = tokio::fs::read(&self.0).await?;
        let mut reader = std::io::BufReader::new(pem.as_slice());

        // Load and return certificate.
        let certs = rustls_pemfile::certs(&mut reader)?;
        Ok(certs.into_iter().map(rustls::Certificate).collect())
    }
}

impl FromStr for TlsCertPath {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

// === impl TlsKeyPath ===

impl TlsKeyPath {
    // Load private key from file.
    async fn load_private_key(&self) -> std::io::Result<rustls::PrivateKey> {
        // Open keyfile.
        let pem = tokio::fs::read(&self.0).await?;
        let mut reader = std::io::BufReader::new(pem.as_slice());

        // Load and return a single private key.
        let keys = rustls_pemfile::rsa_private_keys(&mut reader)?;
        if keys.len() != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "too many private keys",
            ));
        }

        Ok(rustls::PrivateKey(keys[0].clone()))
    }
}

impl FromStr for TlsKeyPath {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

async fn accept_loop<S, B>(
    tcp: TcpListener,
    drain: drain::Watch,
    service: S,
    tls_key: TlsKeyPath,
    tls_certs: TlsCertPath,
) where
    S: Service<hyper::Request<hyper::Body>, Response = hyper::Response<B>> + Clone + Send + 'static,
    S::Error: std::error::Error + Send + Sync,
    S::Future: Send,
    B: hyper::body::HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: std::error::Error + Send + Sync,
{
    loop {
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
                tls_key.clone(),
                tls_certs.clone(),
            )
            .instrument(info_span!("conn", client.ip = %client_addr.ip())),
        );
    }
}

async fn serve_conn<S, B>(
    tcp: TcpStream,
    drain: drain::Watch,
    service: S,
    tls_key: TlsKeyPath,
    tls_certs: TlsCertPath,
) where
    S: Service<hyper::Request<hyper::Body>, Response = hyper::Response<B>> + Send + 'static,
    S::Error: std::error::Error + Send + Sync,
    S::Future: Send,
    B: hyper::body::HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: std::error::Error + Send + Sync,
{
    // Reload the TLS credentials for each connection.
    let tls = match load_tls(&tls_key, &tls_certs).await {
        Ok(tls) => tls,
        Err(error) => {
            info!(%error, "Connection failed");
            return;
        }
    };

    let socket = match tls.accept(tcp).await {
        Ok(s) => s,
        Err(error) => {
            info!(%error, "TLS handshake failed");
            return;
        }
    };

    // Serve the HTTP connection and wait for the drain signal. If a drain is
    // signaled, tell the HTTP connection to terminate gracefully when in-flight
    // requests have completed.
    let mut conn = hyper::server::conn::Http::new().serve_connection(socket, service);
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

/// Load TLS credentials from the provided paths and return a configured TLS acceptor.
async fn load_tls(pk: &TlsKeyPath, crts: &TlsCertPath) -> Result<TlsAcceptor, Error> {
    let key = pk
        .load_private_key()
        .await
        .map_err(Error::TlsKeyReadError)?;

    let certs = crts.load_certs().await.map_err(Error::TlsCertsReadError)?;

    let mut cfg = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(Error::InvalidTlsCredentials)?;
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(TlsAcceptor::from(Arc::new(cfg)))
}
