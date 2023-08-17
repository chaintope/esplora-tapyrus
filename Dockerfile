FROM rust:1.71.0-slim-buster as builder

RUN apt-get update
RUN apt-get install -y clang cmake
RUN apt-get install -y libsnappy-dev git protobuf-compiler
RUN apt-get install -y m4

WORKDIR /app

COPY ./Cargo.toml ./Cargo.toml
COPY ./Cargo.lock ./Cargo.lock
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && cargo build --release

COPY src src
RUN CARGO_BUILD_INCREMENTAL=true cargo build --release

FROM debian:buster-slim

COPY --from=builder /app/target/release/electrs /bin/electrs

# Electrum RPC
EXPOSE 50001 60001

# HTTP
EXPOSE 3000 3001

# Prometheus monitoring
EXPOSE 4224 14224

STOPSIGNAL SIGINT
