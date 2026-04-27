use super::*;
use rustls_pki_types::{pem::PemObject as _, PrivatePkcs1KeyDer, PrivatePkcs8KeyDer};
use std::sync::Arc;
use tokio_rustls::{
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer},
    },
    server::TlsStream,
    TlsAcceptor,
};

pub(super) async fn load_tls(pk: &TlsKeyPath, crts: &TlsCertPath) -> Result<TlsAcceptor, Error> {
    #[cfg(feature = "aws-lc-rs")]
    if tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none() {
        // The only error here is if it's been initialized in between: we can ignore it
        // since our semantic is only to set the default value if it does not exist.
        let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
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

pub(super) async fn accept(
    acceptor: &TlsAcceptor,
    sock: TcpStream,
) -> Result<TlsStream<TcpStream>, std::io::Error> {
    acceptor.accept(sock).await
}

async fn load_certs(
    TlsCertPath(cp): &TlsCertPath,
) -> std::io::Result<Vec<CertificateDer<'static>>> {
    let pem = tokio::fs::read(cp).await?;
    CertificateDer::pem_slice_iter(pem.as_slice())
        .collect::<Result<Vec<_>, _>>()
        .map_err(std::io::Error::other)
}

async fn load_private_key(TlsKeyPath(kp): &TlsKeyPath) -> std::io::Result<PrivateKeyDer<'static>> {
    let pem = tokio::fs::read(kp).await?;

    let mut keys = PrivatePkcs8KeyDer::pem_slice_iter(pem.as_slice())
        .map(|res| res.map(PrivateKeyDer::from))
        .collect::<Result<Vec<_>, _>>()
        .map_err(std::io::Error::other)?;
    if keys.is_empty() {
        keys = PrivatePkcs1KeyDer::pem_slice_iter(pem.as_slice())
            .map(|res| res.map(PrivateKeyDer::from))
            .collect::<Result<Vec<_>, _>>()
            .map_err(std::io::Error::other)?;
    }

    let key = keys
        .pop()
        .ok_or_else(|| std::io::Error::other("could not load private key"))?;
    if !keys.is_empty() {
        return Err(std::io::Error::other("too many private keys"));
    }
    Ok(key)
}
