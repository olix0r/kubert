//! Utilities for Kubernetes controllers built on [`kube`]
//!
//! # Crate Features
//!
//! What functionality is provided by `kubert` is controlled by a number of
//! [Cargo features]. All feature flags are disabled by default, so in order to
//! use the crate, the user must enable one or more feature flags. The following
//! feature flags are available:
//!
//! ### Module Features
//!
//! These features control which of `kubert`'s modules are enabled.
//!
//! - **admin**: Enabled the [`admin`] module.
//! - **client**: Enables the [`client`] module.
//! - **errors**: Enables the [`errors`] module.
//! - **index**: Enables the [`index`] module.
//! - **initialized**: Enables the [`initialized`] module.
//! - **lease**: Enables the [`lease`] module.
//! - **log**: Enables the [`log`] module.
//! - **requeue**: Enables the [`requeue`] module.
//! - **runtime**: Enables the [`runtime`] module. Enabling this feature flag
//!   also enables the **admin**, **client**, **initialized**, and **log**
//!   features.
//! - **server**: Enables the [`server`] module, and server-related
//!   functionality in the [`runtime`] module (if the **runtime** feature is
//!   also enabled).
//! - **shutdown**: Enables the [`shutdown`] module.
//!
//! ### Optional Dependencies
//!
//! These features enable optional dependencies on other crates.
//!
//! - **clap**: Enables support for command-line argument parsing using
//!   [`clap`]. When this feature is enabled, implementations of the
//!   [`clap::Parser`] trait are enabled for the [`AdminArgs`], [`ClientArgs`],
//!   and [`ServerArgs`] types, allowing them to be parsed from command-line
//!   arguments.
//!
//! ### TLS Features
//!
//! These feature flags determine which TLS implementation is used by `kubert`'s
//! [`client`] and [`server`] modules. If neither feature is enabled, `kubert`'s
//! [`client`] module will use whatever TLS implementation is provided by the
//! [`kube-client`] crate's feature flags, and `kubert`'s [`server`] module will
//! panic when starting the server.
//!
//! - **rustls-tls**: Use [`rustls`] as the TLS implementation.
//! - **boring-tls**: Use [BoringSSL] (via the [`boring`] crate) as the TLS
//!   implementation. This feature takes priority over the **rustls-tls**
//!   feature flag. If both are enabled, BoringSSL will be used instead of
//!   Rustls.
//!
//! [`kube`]: https://github.com/kube-rs/kube-rs
//! [Cargo features]: https://doc.rust-lang.org/cargo/reference/features.html
//! [`clap`]: https://crates.io/crates/clap
//! [`clap::Parser`]: https://docs.rs/clap/4/clap/trait.Parser.html
//! [`kube-client`]: https://crates.io/crates/kube-client
//! [`rustls`]: https://crates.io/crates/rustls
//! [BoringSSL]: https://github.com/google/boringssl
//! [`boring`]: https://crates.io/crates/boring

#![deny(warnings, rust_2018_idioms, missing_docs)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "admin")]
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
pub mod admin;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub mod client;

#[cfg(feature = "errors")]
#[cfg_attr(docsrs, doc(cfg(feature = "errors")))]
pub mod errors;

#[cfg(feature = "index")]
#[cfg_attr(docsrs, doc(cfg(feature = "index")))]
pub mod index;

#[cfg(feature = "initialized")]
#[cfg_attr(docsrs, doc(cfg(feature = "initialized")))]
pub mod initialized;

#[cfg(feature = "lease")]
#[cfg_attr(docsrs, doc(cfg(feature = "lease")))]
pub mod lease;

#[cfg(feature = "log")]
#[cfg_attr(docsrs, doc(cfg(feature = "log")))]
pub mod log;

#[cfg(feature = "requeue")]
#[cfg_attr(docsrs, doc(cfg(feature = "requeue")))]
pub mod requeue;

#[cfg(feature = "runtime")]
#[cfg_attr(docsrs, doc(cfg(feature = "runtime")))]
pub mod runtime;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub mod server;

#[cfg(feature = "shutdown")]
#[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
pub mod shutdown;

#[cfg(all(feature = "admin", feature = "clap"))]
pub use self::admin::AdminArgs;

#[cfg(feature = "client")]
pub use self::client::ClientArgs;

#[cfg(feature = "initialized")]
pub use self::initialized::Initialized;

#[cfg(feature = "lease")]
pub use self::lease::LeaseManager;

#[cfg(feature = "log")]
pub use self::log::{LogFilter, LogFormat, LogInitError};

#[cfg(feature = "runtime")]
pub use self::runtime::Runtime;

#[cfg(feature = "server")]
pub use self::server::ServerArgs;
