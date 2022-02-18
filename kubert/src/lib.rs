#![deny(warnings, rust_2018_idioms)]
#![forbid(unsafe_code)]

pub use k8s_openapi as api;

pub use kube;

#[cfg(feature = "client")]
mod client;

#[cfg(all(feature = "client"))]
pub use self::client::{Client, ClientArgs};

#[cfg(feature = "log")]
pub mod log;

#[cfg(feature = "webhook")]
pub mod webhook;
