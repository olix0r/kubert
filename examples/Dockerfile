FROM ghcr.io/linkerd/dev:v43-rust as build
WORKDIR /kubert
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    CARGO_NET_RETRY=10 just-cargo fetch
ARG FEATURES="rustls-tls"
ENV RUSTFLAGS="--cfg tokio_unstable"
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    CARGO_INCREMENTAL=0 just-cargo build \
    --frozen --package=kubert-examples --examples \
    --no-default-features --features=${FEATURES}

FROM gcr.io/distroless/cc-debian12:debug
COPY --from=build /kubert/target/debug/examples/watch-pods /watch-pods
ENTRYPOINT ["/watch-pods"]
