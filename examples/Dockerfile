FROM ghcr.io/linkerd/dev:v34-rust as build
WORKDIR /kubert
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    CARGO_NET_RETRY=10 just-cargo fetch
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    CARGO_INCREMENTAL=0 just-cargo build --frozen --package=kubert-examples --examples

FROM gcr.io/distroless/cc
COPY --from=build /kubert/target/debug/examples/watch-pods /watch-pods
ENTRYPOINT ["/watch-pods"]
