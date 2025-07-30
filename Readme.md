# NoJS Chat

This is a minimal Rust chat server designed to run without JavaScript. It stores user profiles and messages in a local SQLite database and can be deployed as a Tor hidden service.

## Building

```bash
cargo build --release
```

The resulting binary can be found at `target/release/nojs-chat` and does not require any runtime dependencies.

Configuration is read from `config.yml` by default. A sample file is provided as `config.example.yml`.

## Docker

A `Dockerfile` and `docker-compose.yml` are provided. The container runs Tor and exposes the chat as a hidden service.

```bash
docker compose up --build
```

The generated onion address will be written to `tor-data/hostname` after the first start.

When running the binary directly, the chat is available over both HTTP and SSH. Adjust the ports and chat name through `config.yml`.

## Usage

After building, run the server with:

```bash
./target/release/nojs-chat [OPTIONS]
```

Command line flags allow overriding values from the configuration file:

- `-p`, `--port <PORT>` – HTTP port (defaults to `config.yml` or 8080)
- `-s`, `--ssh <PORT>` – SSH port (defaults to `config.yml` or 2222)
- `-n`, `--name <NAME>` – chat name
- `-c`, `--config <FILE>` – path to the configuration file
- `-h`, `--help` – show all flags

Upon start, the binary prints which ports it is listening on and the chosen chat name.
