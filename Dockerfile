FROM rust:1.71.0-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    clang \
    cmake \
    libsnappy-dev \
    git \
    protobuf-compiler \
    m4 \
    libclang-dev \
    libgmp-dev \
    libmpfr-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY ./Cargo.toml ./Cargo.toml
COPY ./Cargo.lock ./Cargo.lock
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && cargo build --release

COPY src src
RUN CARGO_BUILD_INCREMENTAL=true cargo build --release

FROM debian:bookworm-slim

COPY --from=builder /app/target/release/electrs /bin/electrs

# Electrum RPC
EXPOSE 50001 60001

# HTTP
EXPOSE 3000 3001

# Prometheus monitoring
EXPOSE 4224 14224

STOPSIGNAL SIGINT
