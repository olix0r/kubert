# kubert

Rust Kubernetes runtime helpers. Based on [`kube-rs`][krs].

![kubert](https://user-images.githubusercontent.com/240738/154825590-5a94ca46-a453-4037-a738-26663b2c8630.png)

[![Crates.io][crate-badge]][crate-url]
[![Documentation][docs-badge]][docs-url]
[![License](https://img.shields.io/crates/l/kubert)](LICENSE)

[crate-badge]: https://img.shields.io/crates/v/kubert.svg
[crate-url]: https://crates.io/crates/kubert
[docs-badge]: https://docs.rs/kubert/badge.svg
[docs-url]: https://docs.rs/kubert

## Features

* [`clap`](https://docs.rs/clap) command-line interface support;
* A basic admin server with `/ready` and `/live` probe endpoints;
* Optional [`prometheus`][pm] integration;
* A default Kubernetes client;
* Graceful shutdown on `SIGTERM` or `SIGINT` signals;
* An HTTPS server (for admission controllers and API extensions) with
  certificate reloading;
* A utility for maintaining an index derived from watching one or more
  Kubernetes resources types;
* A _requeue_ channel that supports deferring/rescheduling updates (i.e. in case
  a write fails);
* And a [`Runtime`][rt] type that ties it all together!

### Why not `kube-rs`?

The [`kube`][krs] crate is great! And in fact, `kubert` builds on `kube`'s
client and runtime modules. This crate, however, captures some of the repeated
patterns we've encountered building controllers for
[Linkerd](https://github.com/linkerd/linkerd2). It doesn't try to hide
`kube`--though it does reduce boilerplate around initializing watches and caches
(reflectors); and it expects you to schedule work via the `tokio` runtime.

## Examples

This repository includes a simple [example application](./examples) that
demonstrates how to use a `kubert::Runtime`.

Other examples include:

* [Linkerd2 policy controller](https://github.com/linkerd/linkerd2/blob/d4543cd86e427b241ce961b50dd83b1738c0b069/policy-controller/src/main.rs)

## Status

This crate is still fairly experimental, though it's based on production code
from Linkerd; and we plan to use it in Linkerd moving forward.

[krs]: https://docs.rs/kube
[pm]: https://docs.rs/prometheus
[rt]: https://docs.rs/kubert/latest/kubert/runtime/struct.Runtime.html
