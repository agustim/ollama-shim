# multi-stage Dockerfile for building and running ollama-shim

# builder stage
# use a recent Rust image so the 2024 edition is supported
FROM rust:latest as builder
WORKDIR /usr/src/ollama-shim

# copy manifests and source
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# build release binary
RUN cargo build --release

# runtime stage
# use a newer Debian release to get OpenSSL 3 (libssl.so.3)
FROM debian:bookworm-slim

# install runtime dependencies
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libssl3 \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# copy binary from builder
COPY --from=builder /usr/src/ollama-shim/target/release/ollama-shim /usr/local/bin/ollama-shim

# set entrypoint
ENTRYPOINT ["/usr/local/bin/ollama-shim"]
