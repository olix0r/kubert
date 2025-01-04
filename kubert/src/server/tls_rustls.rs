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
    rustls_pemfile::certs(&mut pem.as_slice())
        .map(|res| res.map(CertificateDer::from))
        .collect()
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

    let key = keys.pop().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "could not load private key")
    })?;
    if !keys.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "too many private keys",
        ));
    }
    Ok(key)
}
