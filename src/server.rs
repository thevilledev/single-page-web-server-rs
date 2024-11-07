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
use tokio_rustls::TlsAcceptor;
use tokio::net::TcpListener;
use async_stream::stream;

pub use crate::cli::Args;
pub use crate::metrics::{Metrics, run_metrics_server};

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

#[inline]
fn compress_content(content: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::with_capacity(content.len()), Compression::best());
    encoder.write_all(content.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

pub async fn handle_request(
    req: Request<Body>, 
    state: Arc<AppState>,
    metrics: Arc<Metrics>,
) -> Result<Response<Body>, Infallible> {
    let start = std::time::Instant::now();
    metrics.record_request(req.method().as_str());

    // Check If-None-Match header
    if let Some(if_none_match) = req.headers().get("if-none-match") {
        if if_none_match.as_bytes() == state.etag.as_bytes() {
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

    // Preallocate response builder with common headers
    let response = Response::builder()
        .header("Content-Type", "text/html")
        .header("Cache-Control", "public, max-age=3600, must-revalidate")
        .header("ETag", state.etag.as_bytes())
        .header("Content-Length", if use_compression {
            state.compressed_content_length
        } else {
            state.uncompressed_content_length
        })
        .header("Content-Encoding", if use_compression { "gzip" } else { "identity" })
        .body(Body::from(if use_compression {
            state.compressed_content.clone()
        } else {
            state.uncompressed_content.clone()
        }))
        .unwrap();

    metrics.record_response(
        req.method().as_str(),
        response.status().as_u16(),
        start
    );

    Ok(response)
}

pub async fn run_server(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let metrics = Arc::new(Metrics::new());
    
    // Start metrics server
    let metrics_addr: SocketAddr = format!("{}:{}", args.addr, args.metrics_port)
        .parse()
        .expect("Failed to parse metrics address");
    
    let metrics_clone = metrics.clone();
    tokio::spawn(async move {
        if let Err(e) = run_metrics_server(metrics_clone, metrics_addr).await {
            error!("Metrics server error: {}", e);
        }
    });

    // Read the HTML file at startup
    let html_content = std::fs::read_to_string(&args.index_path)
        .map_err(|e| {
            error!("Failed to read index file: {}", e);
            e
        })?;
    let state = Arc::new(AppState::new(html_content));

    // Calculate optimal buffer size using clamp
    let send_buffer_size = (state.uncompressed_content_length * 2)
        .clamp(32 * 1024, 2* 1024 * 1024);  // Between 32KB and 2MB

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
    socket.set_recv_buffer_size(32 * 1024)?; // Keep receive buffer modest since we expect small requests

    if args.tls {
        info!("Initializing TLS server...");
        run_tls_server(args, addr, state, metrics).await
    } else {
        info!("Initializing plain server...");
        run_plain_server(args, addr, state, metrics).await
    }
    
}

async fn run_tls_server(args: Args, addr: SocketAddr, state: Arc<AppState>, metrics: Arc<Metrics>) -> Result<(), Box<dyn std::error::Error>> {
    let make_svc = make_service_fn(move |_conn| {
        let state = state.clone();
        let metrics = metrics.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle_request(req, state.clone(), metrics.clone())
            }))
        }
    });
    
    let tls_config = crate::tls::TlsConfig::new()?.into_server_config();
    let acceptor = TlsAcceptor::from(tls_config);
    let listener = TcpListener::bind(addr).await?;
    let server = Server::builder(hyper::server::accept::from_stream(stream! {
        loop {
            let (socket, _) = listener.accept().await?;
            yield Ok::<_, std::io::Error>(acceptor.accept(socket).await?);
        }
    }));

    let server = server
        .http1_keepalive(true)
        .http2_keep_alive_interval(Some(std::time::Duration::from_secs(5)))
        .http2_initial_stream_window_size(2 * 1024 * 1024)
        .http2_initial_connection_window_size(4 * 1024 * 1024)
        .http2_adaptive_window(true)
        .serve(make_svc);

    info!("Server running on {}://{}", if args.tls { "https" } else { "http" }, addr);

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

async fn run_plain_server(args: Args, addr: SocketAddr, state: Arc<AppState>, metrics: Arc<Metrics>) -> Result<(), Box<dyn std::error::Error>> {

    let make_svc = make_service_fn(move |_conn| {
        let state = state.clone();
        let metrics = metrics.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle_request(req, state.clone(), metrics.clone())
            }))
        }
    });

    let listener = TcpListener::bind(addr).await?;
    let server = Server::builder(hyper::server::accept::from_stream(stream! {
        loop {
            let (socket, _) = listener.accept().await?;
            yield Ok::<_, std::io::Error>(socket);
        }
    }));

    let server = server
    .http1_keepalive(true)
    .http2_keep_alive_interval(Some(std::time::Duration::from_secs(5)))
    .http2_initial_stream_window_size(2 * 1024 * 1024)
    .http2_initial_connection_window_size(4 * 1024 * 1024)
    .http2_adaptive_window(true)
    .serve(make_svc);

    info!("Server running on {}://{}", if args.tls { "https" } else { "http" }, addr);

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

pub async fn shutdown_signal() {
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