use std::{net::SocketAddr, path::PathBuf};
use tokio_rustls::{rustls, TlsAcceptor};
use tower_service::Service;
use tracing::{debug, error, info, info_span, Instrument};

pub use kube::core::admission::{AdmissionRequest, AdmissionResponse, AdmissionReview};

#[derive(Clone, Debug)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct WebhookArgs {
    #[cfg_attr(feature = "clap", clap(long, default_value = "0.0.0.0/443"))]
    pub webhook_addr: SocketAddr,

    #[cfg_attr(
        feature = "clap",
        clap(long, default_value = "/var/run/linkerd/tls/tls.crt")
    )]
    pub webhook_tls_key: PathBuf,

    #[cfg_attr(
        feature = "clap",
        clap(long, default_value = "/var/run/linkerd/tls/tls.key")
    )]
    pub webhook_tls_certs: PathBuf,
}

impl WebhookArgs {
    /// Bind the specified address and serve the admission controller using the
    /// provided kubernetes client.
    pub async fn serve<S, B>(
        self,
        service: S,
    ) -> std::io::Result<(
        SocketAddr,
        impl std::future::Future<Output = ()> + Send + 'static,
    )>
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
        let tcp = tokio::net::TcpListener::bind(&self.webhook_addr).await?;
        let listen_addr = tcp.local_addr()?;

        let accept_loop = async move {
            loop {
                let socket = match tcp.accept().await {
                    Ok((socket, _)) => socket,
                    Err(error) => {
                        error!(%error, "Failed to accept connection");
                        continue;
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
                let pk = self.webhook_tls_key.clone();
                let crts = self.webhook_tls_certs.clone();
                tokio::spawn(
                    async move {
                        let socket = match accept_tls(&pk, &crts, socket).await {
                            Ok(s) => s,
                            Err(error) => {
                                info!(%error, "Connection failed");
                                return;
                            }
                        };

                        match hyper::server::conn::Http::new()
                            .serve_connection(socket, service)
                            .await
                        {
                            Ok(()) => debug!("Connection closed"),
                            Err(error) => info!(%error, "Connection lost"),
                        };
                    }
                    .instrument(info_span!("webhook", client.ip = %client_addr.ip())),
                );
            }
        };

        Ok((listen_addr, accept_loop))
    }
}

/// Serve an HTTP server for the admission controller on the given TCP
/// connection.
async fn accept_tls(
    pk: &PathBuf,
    crts: &PathBuf,
    socket: tokio::net::TcpStream,
) -> Result<tokio_rustls::server::TlsStream<tokio::net::TcpStream>, String> {
    let tls = {
        let key = load_private_key(&pk)
            .await
            .map_err(|e| format!("failed to load private key: {}", e))?;

        let certs = load_certs(&crts)
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

// Load public certificate from file.
async fn load_certs(filename: &PathBuf) -> Result<Vec<rustls::Certificate>, String> {
    // Open certificate file.
    let pem = tokio::fs::read(filename).await.map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::new(pem.as_slice());

    // Load and return certificate.
    let certs = rustls_pemfile::certs(&mut reader).map_err(|e| e.to_string())?;
    Ok(certs.into_iter().map(rustls::Certificate).collect())
}

// Load private key from file.
async fn load_private_key(filename: &PathBuf) -> Result<rustls::PrivateKey, String> {
    // Open keyfile.
    let pem = tokio::fs::read(filename).await.map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::new(pem.as_slice());

    // Load and return a single private key.
    let keys = rustls_pemfile::rsa_private_keys(&mut reader).map_err(|e| e.to_string())?;
    if keys.len() != 1 {
        return Err("too many private keys".to_string());
    }

    Ok(rustls::PrivateKey(keys[0].clone()))
}
