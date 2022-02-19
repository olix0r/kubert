//! Helpers for configuring and running an HTTPS server, especially for admission controllers and
//! API extensions.

use std::{convert::Infallible, net::SocketAddr, path::PathBuf, str::FromStr};
use thiserror::Error;
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
        S: Clone
            + Send
            + Service<hyper::Request<hyper::Body>, Response = hyper::Response<B>>
            + 'static,
        S::Error: std::error::Error + Send + Sync,
        S::Future: Send,
        B: hyper::body::HttpBody + Send + 'static,
        B::Data: Send,
        B::Error: std::error::Error + Send + Sync,
    {
        let tcp = tokio::net::TcpListener::bind(&self.server_addr)
            .await
            .map_err(|e| Error::Bind(self.server_addr, e))?;
        let local_addr = tcp.local_addr().map_err(Error::LocalAddr)?;

        let server_tls_key = self.server_tls_key.ok_or(Error::NoTlsKey)?;
        let server_tls_certs = self.server_tls_certs.ok_or(Error::NoTlsCerts)?;

        let task = tokio::spawn(async move {
            loop {
                // Wait for the shutdown to be signaled or for the next connection to be accepted.
                let socket = {
                    let drain = drain.clone();
                    tokio::select! {
                        biased;

                        release = drain.signaled() => {
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
                    }
                };

                let client_addr = match socket.peer_addr() {
                    Ok(addr) => addr,
                    Err(error) => {
                        error!(%error, "Failed to get peer address");
                        continue;
                    }
                };

                let service = service.clone();
                let pk = server_tls_key.clone();
                let crts = server_tls_certs.clone();
                let drain = drain.clone();
                tokio::spawn(
                    async move {
                        // Reload the TLS credentials for each connection and terminate TLS.
                        let socket = match accept_tls(&pk, &crts, socket).await {
                            Ok(s) => s,
                            Err(error) => {
                                info!(%error, "Connection failed");
                                return;
                            }
                        };

                        // Serve the HTTP connection and wait for the drain signal. If a drain is
                        // signaled, tell the HTTP connection to terminate gracefully when in-flight
                        // requests have completed.
                        let mut conn =
                            hyper::server::conn::Http::new().serve_connection(socket, service);
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
                    .instrument(info_span!("server", client.ip = %client_addr.ip())),
                );
            }
        });

        Ok(SpawnedServer { local_addr, task })
    }
}

/// Serve an HTTP server for the admission controller on the given TCP
/// connection.
async fn accept_tls(
    pk: &TlsKeyPath,
    crts: &TlsCertPath,
    socket: tokio::net::TcpStream,
) -> Result<tokio_rustls::server::TlsStream<tokio::net::TcpStream>, String> {
    let tls = {
        let key = pk
            .load_private_key()
            .await
            .map_err(|e| format!("failed to load private key: {}", e))?;

        let certs = crts
            .load_certs()
            .await
            .map_err(|e| format!("failed to load certificates: {}", e))?;

        let mut cfg = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| format!("failed to configure TLS: {}", e))?;

        cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        TlsAcceptor::from(std::sync::Arc::new(cfg))
    };

    tls.accept(socket)
        .await
        .map_err(|e| format!("TLS handshake failed: {}", e))
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
    async fn load_certs(&self) -> Result<Vec<rustls::Certificate>, String> {
        // Open certificate file.
        let pem = tokio::fs::read(&self.0).await.map_err(|e| e.to_string())?;
        let mut reader = std::io::BufReader::new(pem.as_slice());

        // Load and return certificate.
        let certs = rustls_pemfile::certs(&mut reader).map_err(|e| e.to_string())?;
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
    async fn load_private_key(&self) -> Result<rustls::PrivateKey, String> {
        // Open keyfile.
        let pem = tokio::fs::read(&self.0).await.map_err(|e| e.to_string())?;
        let mut reader = std::io::BufReader::new(pem.as_slice());

        // Load and return a single private key.
        let keys = rustls_pemfile::rsa_private_keys(&mut reader).map_err(|e| e.to_string())?;
        if keys.len() != 1 {
            return Err("too many private keys".to_string());
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
