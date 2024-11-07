# single-page-web-server-rs

This is a simple single-page web server written in Rust. It serves a static HTML file and can be configured to use a different listen address and port.

I worked on [thevilledev/single-page-web-server](https://github.com/thevilledev/single-page-web-server) project in 2014, which was 10 years ago. The idea is the same but implemented in C. Use case was HTTP sorry-servers for load balancing. Make it fast only for that use case, without any dependencies.

Rewrite in Rust.

## Usage

```bash
$ cargo run -- --help
Usage: single-page-web-server-rs [OPTIONS]

Options:
      --index-path <INDEX_PATH>      Path to the index HTML file [env: WEB_INDEX_PATH=] [default: index.html]
      --port <PORT>                  Port to listen on [env: WEB_PORT=] [default: 3000]
      --addr <ADDR>                  Address to bind to [env: WEB_ADDR=] [default: 127.0.0.1]
      --metrics-port <METRICS_PORT>  Metrics server port [env: METRICS_PORT=] [default: 3001]
      --tls                          Enable TLS with self-signed certificate [env: ENABLE_TLS=]
  -h, --help                         Print help
  -V, --version                      Print version
  ```

Examples:

```bash
$ cargo run -- --addr 0.0.0.0 --port 8080
Server running on http://0.0.0.0:8080
```

```bash
$ curl http://localhost:8080
foo
```

Query metrics:

```bash
$ curl http://localhost:3001/metrics
```

## Pre-built binaries

Pre-built binaries are available in the [releases](https://github.com/thevilledev/single-page-web-server-rs/releases) page for the following platforms:

- Linux (x86_64)
- macOS (Intel and Apple Silicon)
- Windows (x86_64)

## Container image

Container image is available on [GHCR](https://github.com/thevilledev/single-page-web-server-rs/pkgs/container/single-page-web-server-rs).

```bash
$ docker run -p 3000:3000 --rm ghcr.io/thevilledev/single-page-web-server-rs:latest
Server running on http://0.0.0.0:3000
```

```bash
$ curl http://localhost:8080
foo
```

Replace the `index.html` with a file of your choice by a volume mount.

```bash
$ docker run -p 3000:3000 --rm -v $(pwd)/index.html:/app/index.html ghcr.io/thevilledev/single-page-web-server-rs:latest
Server running on http://0.0.0.0:3000
```

```bash
$ curl http://localhost:8080
bar
```

## Customise via environment variables

```bash
$ WEB_PORT=8080 cargo run
Server running on http://127.0.0.1:8080
```

```bash
$ WEB_ADDR=0.0.0.0 WEB_PORT=8080 cargo run
Server running on http://0.0.0.0:8080
```

```bash
$ WEB_INDEX_PATH=index.html cargo run
Server running on http://127.0.0.1:3000
```

## License

MIT