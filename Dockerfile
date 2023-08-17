FROM rust:1.71.0-slim-buster

RUN apt-get update
RUN apt-get install -y clang cmake
RUN apt-get install -y libsnappy-dev git protobuf-compiler
RUN apt-get install -y m4

WORKDIR /var/lib/electrs
COPY ./ /var/lib/electrs

RUN cargo build --release
RUN cargo install --path .

# Electrum RPC
EXPOSE 50001 60001

# HTTP
EXPOSE 3000 3001

# Prometheus monitoring
EXPOSE 4224 14224

STOPSIGNAL SIGINT
