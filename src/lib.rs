use clap::Parser;
use hyper::{Body, Request, Response};
use std::convert::Infallible;
use std::sync::Arc;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;
use tokio::signal;

pub use crate::server::{AppState, handle_request};
pub use crate::cli::Args;

pub mod cli {
    use super::*;

    #[derive(Parser, Debug)]
    #[command(author, version, about, long_about = None)]
    pub struct Args {
        /// Path to the index HTML file
        #[arg(long, default_value = "index.html", env = "WEB_INDEX_PATH")]
        pub index_path: String,

        /// Port to listen on
        #[arg(long, default_value_t = 3000, env = "WEB_PORT")]
        pub port: u16,

        /// Address to bind to
        #[arg(long, default_value = "127.0.0.1", env = "WEB_ADDR")]
        pub addr: String,
    }
}

pub mod server {
    use super::*;
    use hyper::Server;
    use hyper::service::{make_service_fn, service_fn};
    use std::net::SocketAddr;
    use tokio::net::TcpSocket;
    use tracing::{info, error};
    use crate::cli::Args;

    pub struct AppState {
        pub html_content: Arc<String>,
        pub etag: String,
        pub content_length: usize,
        pub compressed: Arc<Vec<u8>>,
    }

    impl AppState {
        pub fn new(content: String) -> Self {
            let digest = md5::compute(&content);
            let etag = format!("\"{:x}\"", digest);
            let compressed = Arc::new(compress_content(&content));
            AppState {
                content_length: content.len(),
                etag,
                compressed,
                html_content: Arc::new(content),
            }
        }
    }


    fn compress_content(content: &str) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(content.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }

    pub async fn handle_request(req: Request<Body>, state: Arc<AppState>) -> Result<Response<Body>, Infallible> {
        // Check If-None-Match header
        if let Some(if_none_match) = req.headers().get("if-none-match") {
            if if_none_match.to_str().unwrap_or("") == state.etag {
                return Ok(Response::builder()
                    .status(304)
                    .body(Body::empty())
                    .unwrap());
            }
        }

        // Check if client accepts gzip
        let use_compression = req.headers()
            .get("accept-encoding")
            .and_then(|val| val.to_str().ok())
            .map_or(false, |val| val.contains("gzip"));

        let mut builder = Response::builder()
            .header("Content-Type", "text/html")
            .header("Cache-Control", "public, max-age=31536000, immutable")
            .header("ETag", &state.etag)
            .header("Content-Length", if use_compression {
                state.compressed.len()
            } else {
                state.content_length
            });

        // Add compression header if used
        if use_compression {
            builder = builder.header("Content-Encoding", "gzip");
        }

        Ok(builder
            .body(Body::from(if use_compression {
                state.compressed.as_ref().clone()
            } else {
                state.html_content.as_bytes().to_vec()
            }))
            .unwrap())
    }

    pub async fn run_server(args: Args) -> Result<(), Box<dyn std::error::Error>> {
        // Read the HTML file at startup
        let html_content = std::fs::read_to_string(&args.index_path)
            .map_err(|e| {
                error!("Failed to read index file: {}", e);
                e
            })?;
        let state = Arc::new(AppState::new(html_content));

        // Calculate optimal buffer size using clamp
        let send_buffer_size = (state.html_content.len() * 2)
            .clamp(16 * 1024, 1024 * 1024);  // Between 16KB and 1MB

        // Configure the server address
        let addr: SocketAddr = format!("{}:{}", args.addr, args.port)
            .parse()
            .expect("Failed to parse address");
        let socket = if addr.is_ipv6() {
            TcpSocket::new_v6()?
        } else {
            TcpSocket::new_v4()?
        };

        // Set optimized buffer sizes
        socket.set_send_buffer_size(send_buffer_size.try_into().unwrap())?;
        socket.set_recv_buffer_size(16 * 1024)?; // Keep receive buffer modest since we expect small requests

        // Create the service
        let make_svc = make_service_fn(move |_conn| {
            let state = state.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    handle_request(req, state.clone())
                }))
            }
        });

        // Create and configure the server
        let server = Server::bind(&addr)
            .http1_keepalive(true)
            .http2_keep_alive_interval(Some(std::time::Duration::from_secs(5)))
            .tcp_nodelay(true)
            .serve(make_svc);

        info!("Server running on http://{}", addr);

        // Handle graceful shutdown
        let graceful = server.with_graceful_shutdown(shutdown_signal());

        // Run the server
        if let Err(e) = graceful.await {
            error!("Server error: {}", e);
            return Err(e.into());
        }

        info!("Server shutdown complete");
        Ok(())
    }

    async fn shutdown_signal() {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => info!("Received Ctrl+C signal"),
            _ = terminate => info!("Received terminate signal"),
        }
    }
}