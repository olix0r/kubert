use super::*;
use std::sync::Arc;
use tokio_rustls::{
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer},
    },
    server::TlsStream,
    TlsAcceptor,
};

pub(in crate::server) async fn load_tls(
    pk: &TlsKeyPath,
    crts: &TlsCertPath,
) -> Result<TlsAcceptor, Error> {
    if tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none() {
        // The only error here is if it's been initialized in between: we can ignore it
        // since our semantic is only to set the default value if it does not exist.
        #[cfg(feature = "rustls-tls-aws-lc-rs")]
        let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
        #[cfg(feature = "rustls-tls-ring")]
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    }

    let key = load_private_key(pk).await.map_err(Error::TlsKeyReadError)?;
    let certs = load_certs(crts).await.map_err(Error::TlsCertsReadError)?;
    let mut cfg = rustls::ServerConfig::builder()
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

async fn load_certs(
    TlsCertPath(cp): &TlsCertPath,
) -> std::io::Result<Vec<CertificateDer<'static>>> {
    let pem = tokio::fs::read(cp).await?;
    rustls_pemfile::certs(&mut pem.as_slice()).collect()
}

async fn load_private_key(TlsKeyPath(kp): &TlsKeyPath) -> std::io::Result<PrivateKeyDer<'static>> {
    let pem = tokio::fs::read(kp).await?;

    let mut keys = rustls_pemfile::pkcs8_private_keys(&mut pem.as_slice())
        .map(|res| res.map(PrivateKeyDer::from))
        .collect::<Result<Vec<_>, _>>()?;
    if keys.is_empty() {
        keys = rustls_pemfile::rsa_private_keys(&mut pem.as_slice())
            .map(|res| res.map(PrivateKeyDer::from))
            .collect::<Result<Vec<_>, _>>()?;
    }

    let key = keys
        .pop()
        .ok_or_else(|| std::io::Error::other("could not load private key"))?;
    if !keys.is_empty() {
        return Err(std::io::Error::other("too many private keys"));
    }
    Ok(key)
}
