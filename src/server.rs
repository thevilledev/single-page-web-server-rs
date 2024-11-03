use flate2::Compression;
use flate2::write::GzEncoder;
use hyper::Server;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response};
use hyper::body::Bytes;
use std::convert::Infallible;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpSocket;
use tokio::signal;
use tracing::{info, error};

pub use crate::cli::Args;

#[repr(align(64))]
pub struct AppState {
    pub etag: Box<str>,                     // 16 bytes
    pub compressed_content_length: usize,   // 8 bytes
    pub uncompressed_content_length: usize, // 8 bytes
    pub compressed_content: Bytes,          // 32 bytes
    pub uncompressed_content: Bytes,        // 32 bytes
}

impl AppState {
    pub fn new(content: String) -> Self {
        let digest = md5::compute(&content);
        let etag = format!("\"{:x}\"", digest).into_boxed_str();
        let compressed_content = Bytes::from(compress_content(&content));
        let uncompressed_content = Bytes::from(content.into_bytes());
        AppState {
            compressed_content_length: compressed_content.len(),
            uncompressed_content_length: uncompressed_content.len(),
            etag,
            compressed_content,
            uncompressed_content,
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
        if if_none_match.as_bytes().eq_ignore_ascii_case(state.etag.as_bytes()) {
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
        .header("Cache-Control", "public, max-age=3600, must-revalidate")
        .header("ETag", state.etag.as_bytes())
        .header("Content-Length", if use_compression {
            state.compressed_content_length
        } else {
            state.uncompressed_content_length
        });

    // Add compression header if used
    if use_compression {
        builder = builder.header("Content-Encoding", "gzip");
    }

    Ok(builder
        .body(Body::from(if use_compression {
            state.compressed_content.clone()
        } else {
            state.uncompressed_content.clone()
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
    let send_buffer_size = (state.uncompressed_content_length * 2)
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
        .http2_initial_stream_window_size(1024 * 1024)
        .http2_initial_connection_window_size(1024 * 1024 * 2)
        .http2_adaptive_window(true)
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