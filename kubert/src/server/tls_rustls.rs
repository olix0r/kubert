use super::*;
use std::sync::Arc;
use tokio_rustls::{rustls, server::TlsStream, TlsAcceptor};

pub(in crate::server) async fn load_tls(
    pk: &TlsKeyPath,
    crts: &TlsCertPath,
) -> Result<TlsAcceptor, Error> {
    let key = pk
        .load_private_key()
        .await
        .map_err(Error::TlsKeyReadError)?;

    let certs = crts.load_certs().await.map_err(Error::TlsCertsReadError)?;

    let mut cfg = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|err| Error::InvalidTlsCredentials(Box::new(err)))?;
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

pub(in crate::server) async fn accept(
    acceptor: &TlsAcceptor,
    sock: TcpStream,
) -> Result<TlsStream<TcpStream>, std::io::Error> {
    acceptor.accept(sock).await
}

// === impl TlsCertPath ===

impl TlsCertPath {
    // Load public certificate from file
    async fn load_certs(&self) -> std::io::Result<Vec<rustls::Certificate>> {
        // Open certificate file.
        let pem = tokio::fs::read(&self.0).await?;

        // Load and return certificate.
        let certs = rustls_pemfile::certs(&mut pem.as_slice())?;
        Ok(certs.into_iter().map(rustls::Certificate).collect())
    }
}

// === impl TlsKeyPath ===

impl TlsKeyPath {
    async fn load_private_key(&self) -> std::io::Result<rustls::PrivateKey> {
        // Open keyfile.
        let pem = tokio::fs::read(&self.0).await?;

        // Load and return a single private key.
        let mut keys = rustls_pemfile::pkcs8_private_keys(&mut pem.as_slice())?;

        if keys.is_empty() {
            keys = rustls_pemfile::rsa_private_keys(&mut pem.as_slice())?;

            if keys.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "could not load private key",
                ));
            }
        }

        if keys.len() > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "too many private keys",
            ));
        }

        Ok(rustls::PrivateKey(keys[0].clone()))
    }
}
