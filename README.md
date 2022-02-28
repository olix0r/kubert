# kubert

Rust Kubernetes runtime helpers. Based on [`kube-rs`][krs].

![kubert](https://user-images.githubusercontent.com/240738/154825590-5a94ca46-a453-4037-a738-26663b2c8630.png)

[![Crates.io][crate-badge]][crate-url]
[![Documentation][docs-badge]][docs-url]
[![License][lic-badge]](LICENSE)

[crate-badge]: https://img.shields.io/crates/v/kubert.svg
[crate-url]: https://crates.io/crates/kube
[docs-badge]: https://docs.rs/kubert/badge.svg
[docs-url]: https://docs.rs/kubert
[docs-url]: https://img.shields.io/crates/l/kubert
[lic-badge]: https://img.shields.io/crates/l/kubert

## Features

* [`clap`](https://docs.rs/clap) command-line interface support
* A basic admin server with `/ready` and `/live` probe endpoints;
* A default Kubernetes client
* Graceful shutdown on `SIGTERM` or `SIGINT` signals
* An HTTPS server (for admission controllers and API extensions) with certificate reloading
* A _requeue_ channel that supports deferring/rescheduling updates (i.e. in case a write fails).
* A [`Runtime`][rt] type that ties it all together!

### Why not `kube-rs`?

The [`kube`][krs] crate is great! And in fact, `kubert` builds on `kube`'s client and runtime
modules. This crate, however, captures some of the repeated patterns we've encountered building
controllers for [Linkerd](https://github.com/linkerd/linkerd2). It doesn't try to hide
`kube`--though it does reduce boilerplate around initializing watches and caches (reflectors); and
it expects you to schedule work via the `tokio` runtime.

## Status

This crate is still fairly experimental, though it's based on production code from Linkerd; and we
plan to use it in Linkerd moving forward.

[krs]: https://docs.rs/kube
[rt]: https://docs.rs/kubert/latest/kubert/runtime/struct.Runtime.html
