//! Utilities for Kubernetes controllers built on [`kube`]
//!
//! [`kube`]: https://github.com/kube-rs/kube-rs

#![deny(warnings, rust_2018_idioms, missing_docs)]
#![forbid(unsafe_code)]

#[cfg(feature = "client")]
pub mod client;

#[cfg(all(feature = "client"))]
pub use self::client::ClientArgs;

#[cfg(feature = "log")]
pub mod log;

#[cfg(feature = "shutdown")]
pub mod shutdown;

#[cfg(feature = "server")]
pub mod server;
