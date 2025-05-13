FROM rust:slim

RUN apt update && \
    apt install -y git iproute2

RUN git clone https://github.com/libp2p/rust-libp2p.git

WORKDIR /rust-libp2p
RUN cd examples/relay-server && \
    cargo build

RUN cd examples/dcutr && \
    cargo build

RUN cargo install libp2p-lookup