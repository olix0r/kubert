use super::*;
use once_cell::sync::Lazy;
use openssl::{
    error::ErrorStack,
    pkey::{PKey, Private},
    ssl::{self, Ssl},
    x509::X509,
};
use std::pin::Pin;
use tokio_openssl::SslStream;

pub(in crate::server) type TlsAcceptor = ssl::SslAcceptor;

#[derive(Debug, thiserror::Error)]
pub enum AcceptError {
    #[error("failed to construct SSL from acceptor context: {0}")]
    Ssl(#[source] ErrorStack),
    #[error("failed to construct SslStream from SSL: {0}")]
    Stream(#[source] ErrorStack),
    #[error("failed to accept TLS connection: {0}")]
    Accept(#[from] ssl::Error),
}

pub(in crate::server) async fn accept(
    acceptor: &TlsAcceptor,
    sock: TcpStream,
) -> Result<SslStream<TcpStream>, AcceptError> {
    let ssl = Ssl::new(acceptor.context()).map_err(AcceptError::Ssl)?;

    let mut stream = SslStream::new(ssl, sock).map_err(AcceptError::Stream)?;

    Pin::new(&mut stream).accept().await?;
    Ok(stream)
}

pub(in crate::server) async fn load_tls(
    pk: &TlsKeyPath,
    crts: &TlsCertPath,
) -> Result<TlsAcceptor, Error> {
    let key = pk
        .load_private_key()
        .await
        .map_err(Error::TlsKeyReadError)?;

    let certs = crts.load_certs().await.map_err(Error::TlsCertsReadError)?;

    configure(key, certs).map_err(|error| Error::InvalidTlsCredentials(Box::new(error)))
}

fn configure(key: PKey<Private>, certs: Vec<X509>) -> Result<TlsAcceptor, ErrorStack> {
    // mozilla_intermediate_v5 is the only variant that enables TLSv1.3, so we use that.
    let mut conn = {
        let method = ssl::SslMethod::tls_server();
        ssl::SslAcceptor::mozilla_intermediate_v5(method)?
    };

    // Disable client auth.
    conn.set_verify(ssl::SslVerifyMode::NONE);
    conn.set_private_key(&key)?;
    conn.set_certificate(&certs[0])?;

    for c in certs.iter().skip(1) {
        conn.add_extra_chain_cert(c.to_owned())?;
    }

    conn.set_alpn_protos(&ALPN_PROTOCOLS)?;

    Ok(conn.build())
}

/// ALPN protocols encoded as length-prefixed strings.
///
/// `boring` requires that the list of protocols be encoded in the wire format.
static ALPN_PROTOCOLS: Lazy<Vec<u8>> = Lazy::new(|| {
    let protocols: &[&[u8]] = &[b"h2", b"http/1.1"];
    // Allocate a buffer to hold the encoded protocols.
    let mut bytes = {
        // One additional byte for each protocol's length prefix.
        let cap = protocols.len() + protocols.iter().map(|p| p.len()).sum::<usize>();
        Vec::with_capacity(cap)
    };

    // Encode each protocol as a length-prefixed string.
    for p in protocols {
        if p.is_empty() {
            continue;
        }
        // Since we only call this with "h2" and "http/1.1", this assertion
        // should never be hit.
        debug_assert!(p.len() <= 255, "ALPN protocols must be less than 256 bytes");
        bytes.push(p.len() as u8);
        bytes.extend_from_slice(p);
    }

    bytes
});

// === impl TlsCertPath ===

impl TlsCertPath {
    // Load public certificate from file
    async fn load_certs(&self) -> std::io::Result<Vec<X509>> {
        // Open certificate file.
        let pem = tokio::fs::read(&self.0).await?;

        // Load and return certificate.
        let certs = X509::stack_from_pem(&pem)?;
        Ok(certs)
    }
}

// === impl TlsKeyPath ===

impl TlsKeyPath {
    async fn load_private_key(&self) -> std::io::Result<PKey<Private>> {
        // Open keyfile.
        let pem = tokio::fs::read(&self.0).await?;

        // Load and return a single private key. The keyfile should be
        // PEM-encoded.
        // TODO(eliza): Potentially, we may want to support both PEM-encoded and
        // DER-encoded keyfiles, and decide whether to use
        // `PKey::private_key_from_pem` or `PKey::private_key_from_pkcs8` based
        // on the filename extension.
        Ok(PKey::private_key_from_pem(&pem)?)
    }
}
