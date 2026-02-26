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
FROM debian:buster-slim
# copy binary from builder
COPY --from=builder /usr/src/ollama-shim/target/release/ollama-shim /usr/local/bin/ollama-shim

# set entrypoint
ENTRYPOINT ["/usr/local/bin/ollama-shim"]
