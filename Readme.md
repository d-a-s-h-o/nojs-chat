# NoJS Chat

This is a minimal Rust chat server designed to run without JavaScript. It stores user profiles and messages in a local SQLite database and can be deployed as a Tor hidden service.

## Building

```bash
cargo build --release
```

The resulting binary can be found at `target/release/nojs-chat` and does not require any runtime dependencies.

Configuration is read from `config.yml`. A sample file is provided as `config.example.yml`.

## Docker

A `Dockerfile` and `docker-compose.yml` are provided. The container runs Tor and exposes the chat as a hidden service.

```bash
docker compose up --build
```

The generated onion address will be written to `tor-data/hostname` after the first start.

When running the binary directly, the chat is available over both HTTP and SSH. Adjust the ports and chat name through `config.yml`.
