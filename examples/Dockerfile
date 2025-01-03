FROM ghcr.io/linkerd/dev:v44-rust as build
WORKDIR /kubert
COPY . .
ENV CARGO_NET_RETRY=10 \
    CARGO_INCREMENTAL=0 \
    RUSTFLAGS="--cfg tokio_unstable"
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    just-cargo fetch
ARG FEATURES=""
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    just-cargo build \
    --package=kubert-examples --examples \
    --no-default-features --features=${FEATURES}

FROM gcr.io/distroless/cc-debian12:debug
COPY --from=build /kubert/target/debug/examples/watch-pods /watch-pods
ENTRYPOINT ["/watch-pods"]
