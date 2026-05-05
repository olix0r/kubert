use super::*;
use rustls_pki_types::{
    pem::{Error as PemError, PemObject as _},
    PrivatePkcs1KeyDer, PrivatePkcs8KeyDer,
};
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
        .map_err(pem_error_into_io_error)
}

async fn load_private_key(TlsKeyPath(kp): &TlsKeyPath) -> std::io::Result<PrivateKeyDer<'static>> {
    let pem = tokio::fs::read(kp).await?;

    let mut keys = PrivatePkcs8KeyDer::pem_slice_iter(pem.as_slice())
        .map(|res| res.map(PrivateKeyDer::from))
        .collect::<Result<Vec<_>, _>>()
        .map_err(pem_error_into_io_error)?;
    if keys.is_empty() {
        keys = PrivatePkcs1KeyDer::pem_slice_iter(pem.as_slice())
            .map(|res| res.map(PrivateKeyDer::from))
            .collect::<Result<Vec<_>, _>>()
            .map_err(pem_error_into_io_error)?;
    }

    let key = keys
        .pop()
        .ok_or_else(|| std::io::Error::other("could not load private key"))?;
    if !keys.is_empty() {
        return Err(std::io::Error::other("too many private keys"));
    }
    Ok(key)
}

/// Converts a [`rustls_pki_types::pem::Error`] into a [`std::io::Error`].
///
/// This function exists to preserve identical error semantics and error formatting with the
/// behavior previously exhibited by `rustls_pemfile`. This presents errors due to missing section
/// end markers, illegal section starts, and decoding errors as [`std::io::ErrorKind::InvalidData`]
/// errors. Other pemfile errors are reported as "other" i/o errors.
fn pem_error_into_io_error(error: PemError) -> std::io::Error {
    use std::io::{self, ErrorKind};

    match error {
        PemError::MissingSectionEnd { end_marker } => io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "section end {:?} missing",
                String::from_utf8_lossy(&end_marker)
            ),
        ),

        PemError::IllegalSectionStart { line } => io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "illegal section start: {:?}",
                String::from_utf8_lossy(&line)
            ),
        ),

        PemError::Base64Decode(err) => io::Error::new(ErrorKind::InvalidData, err),
        error => io::Error::other(error),
    }
}
