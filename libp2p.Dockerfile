# Builder stage
FROM rust:slim AS builder

COPY chat /workspace/chat
COPY relay-server /workspace/relay-server

WORKDIR /workspace/chat
RUN cargo build --release

WORKDIR /workspace/relay-server
RUN cargo build --release

# Final image
FROM debian:stable-slim

RUN apt update && \
    apt install -y iproute2 && \
    rm -rf /var/lib/apt/lists/*

# Copy relay-server and dcutr binaries
COPY --from=builder /workspace/chat/target/release/chat /usr/local/bin/
COPY --from=builder /workspace/relay-server/target/release/relay-server /usr/local/bin/

ENTRYPOINT ["/bin/bash"]