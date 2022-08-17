ARG RUST_VERSION=1.63

FROM docker.io/library/rust:${RUST_VERSION}-bullseye as build
WORKDIR /kubert
COPY . .
RUN cargo fetch
RUN cargo build --frozen --package=kubert-examples --examples

FROM gcr.io/distroless/cc
COPY --from=build /kubert/target/debug/examples/watch-pods /usr/local/bin/
