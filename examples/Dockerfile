ARG RUST_VERSION=1.56.1

FROM docker.io/library/rust:${RUST_VERSION}-bullseye as build
WORKDIR /kubert
COPY . .
RUN --mount=type=cache,target=target \
    --mount=type=cache,from=rust:1.56.1,source=/usr/local/cargo,target=/usr/local/cargo \
    cargo fetch --locked
RUN --mount=type=cache,target=target \
    --mount=type=cache,from=rust:1.56.1,source=/usr/local/cargo,target=/usr/local/cargo \
    cargo build --frozen --package kubert-examples --examples \
        && mv target/debug/examples/watch-pods /tmp/watch-pods

FROM gcr.io/distroless/cc
COPY --from=build /tmp/watch-pods /usr/local/bin/
