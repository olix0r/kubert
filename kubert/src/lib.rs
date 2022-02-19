#![deny(warnings, rust_2018_idioms)]
#![forbid(unsafe_code)]

#[cfg(feature = "client")]
mod client;

#[cfg(all(feature = "client"))]
pub use self::client::ClientArgs;

#[cfg(feature = "log")]
pub mod log;

#[cfg(feature = "shutdown")]
pub mod shutdown;

#[cfg(feature = "server")]
pub mod server;
