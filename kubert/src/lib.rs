//! Utilities for Kubernetes controllers built on [`kube`]
//!
//! [`kube`]: https://github.com/kube-rs/kube-rs

#![deny(warnings, rust_2018_idioms, missing_docs)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(any(feature = "client"))))]
pub mod client;

#[cfg(all(feature = "client"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "client"))))]
pub use self::client::ClientArgs;

#[cfg(feature = "log")]
#[cfg_attr(docsrs, doc(cfg(any(feature = "log"))))]
pub mod log;

#[cfg(feature = "shutdown")]
#[cfg_attr(docsrs, doc(cfg(any(feature = "shutdown"))))]
pub mod shutdown;

#[cfg(feature = "requeue")]
#[cfg_attr(docsrs, doc(cfg(any(feature = "requeue"))))]
pub mod requeue;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(any(feature = "shutdown"))))]
pub mod server;
