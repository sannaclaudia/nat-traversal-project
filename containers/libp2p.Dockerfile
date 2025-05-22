# Builder stage
FROM rust:slim AS builder

RUN apt update && \
    apt install -y git

RUN git clone https://github.com/libp2p/rust-libp2p.git

WORKDIR /rust-libp2p/examples/relay-server
RUN cargo build --release

WORKDIR /rust-libp2p/examples/dcutr
RUN cargo build --release

WORKDIR /rust-libp2p
RUN cargo install libp2p-lookup

# Final image
FROM debian:stable-slim

RUN apt update && \
    apt install -y iproute2 && \
    rm -rf /var/lib/apt/lists/*

# Copy relay-server and dcutr binaries
COPY --from=builder /rust-libp2p/target/release/relay-server-example /usr/local/bin/
COPY --from=builder /rust-libp2p/target/release/dcutr-example /usr/local/bin/
# Copy libp2p-lookup binary
COPY --from=builder /usr/local/cargo/bin/libp2p-lookup /usr/local/bin/

ENTRYPOINT ["/bin/bash"]