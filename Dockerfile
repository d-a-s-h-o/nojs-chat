FROM rust:1.76-slim as builder
WORKDIR /app
COPY . .
RUN apt-get update && apt-get install -y pkg-config libssl-dev build-essential sqlite3 libsqlite3-dev && \
    cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y tor && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/nojs-chat /usr/local/bin/nojs-chat
COPY torrc /etc/tor/torrc
EXPOSE 8080
CMD tor & sleep 5 && /usr/local/bin/nojs-chat
