//! Utilities for Kubernetes controllers built on [`kube`]
//!
//! [`kube`]: https://github.com/kube-rs/kube-rs

#![deny(warnings, rust_2018_idioms, missing_docs)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "admin")]
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
pub mod admin;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub mod client;

#[cfg(feature = "log")]
#[cfg_attr(docsrs, doc(cfg(feature = "log")))]
pub mod log;

#[cfg(feature = "requeue")]
#[cfg_attr(docsrs, doc(cfg(feature = "requeue")))]
pub mod requeue;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub mod server;

#[cfg(feature = "shutdown")]
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
pub mod shutdown;

#[cfg(all(feature = "admin", feature = "clap"))]
pub use self::admin::AdminArgs;

#[cfg(all(feature = "client"))]
pub use self::client::ClientArgs;
