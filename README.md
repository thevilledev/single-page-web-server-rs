# single-page-web-server-rs

This is a simple single-page web server written in Rust. It serves a static HTML file and can be configured to use a different listen address and port.

I worked on [thevilledev/single-page-web-server](https://github.com/thevilledev/single-page-web-server) project in 2014, which was 10 years ago. The idea is the same but implemented in C. Use case was HTTP sorry-servers for load balancing. Make it fast only for that use case, without any dependencies.

Rewrite in Rust.

## Usage

```bash
$ cargo run -- --addr 0.0.0.0 --port 8080
Server running on http://0.0.0.0:8080
```

```bash
$ curl http://localhost:8080
foo
```

## Pre-built binaries

Pre-built binaries are available in the [releases](https://github.com/thevilledev/single-page-web-server-rs/releases) page for the following platforms:

- Linux (x86_64)
- macOS (Intel and Apple Silicon)
- Windows (x86_64)

## Container image

Container image is available on [GHCR](https://github.com/thevilledev/single-page-web-server-rs/pkgs/container/single-page-web-server-rs).

```bash
$ docker run --rm ghcr.io/thevilledev/single-page-web-server-rs:latest
Server running on http://0.0.0.0:8080
```

## License

MIT