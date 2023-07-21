use super::*;
use std::{convert::Infallible, path::PathBuf, str::FromStr};

#[cfg(all(feature = "rustls-tls", not(feature = "boring-tls")))]
#[path = "tls/rustls.rs"]
mod inner;

#[cfg(feature = "boring-tls")]
#[path = "tls/boring.rs"]
mod inner;
pub(super) use self::inner::*;

/// The path to the server's TLS private key
#[derive(Clone, Debug)]
#[cfg_attr(
    docsrs,
    doc(cfg(all(
        feature = "server",
        any(feature = "rustls-tls", feature = "boring-tls")
    )))
)]
pub struct TlsKeyPath(PathBuf);

/// The path to the server's TLS certificate bundle
#[derive(Clone, Debug)]
#[cfg_attr(
    docsrs,
    doc(cfg(all(
        feature = "server",
        any(feature = "rustls-tls", feature = "boring-tls")
    )))
)]
pub struct TlsCertPath(PathBuf);

impl FromStr for TlsCertPath {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

// === impl TlsKeyPath ===

impl FromStr for TlsKeyPath {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}
