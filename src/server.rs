use bytes::Bytes;
use flate2::Compression;
use flate2::write::GzEncoder;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use socket2::{Domain, Protocol, Socket, Type};
use std::convert::Infallible;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, warn};

pub use crate::cli::Args;
pub use crate::metrics::{Metrics, OptionalMetrics, run_metrics_server};

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

/// Create a TCP listener with optional SO_REUSEPORT support and optimized buffer sizes
async fn create_listener(
    addr: SocketAddr,
    reuse_port: bool,
    send_buffer_size: usize,
) -> std::io::Result<TcpListener> {
    let domain = if addr.is_ipv6() {
        Domain::IPV6
    } else {
        Domain::IPV4
    };

    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;

    // Set SO_REUSEADDR (always useful)
    socket.set_reuse_address(true)?;

    // Set SO_REUSEPORT on supported platforms (Linux, macOS, FreeBSD)
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    if reuse_port {
        socket.set_reuse_port(true)?;
        info!("SO_REUSEPORT enabled for multi-core scaling");
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
    if reuse_port {
        warn!("SO_REUSEPORT not supported on this platform, ignoring --reuse-port flag");
    }

    // Set optimized buffer sizes
    socket.set_send_buffer_size(send_buffer_size)?;
    socket.set_recv_buffer_size(32 * 1024)?; // Keep receive buffer modest since we expect small requests

    // Set non-blocking before binding
    socket.set_nonblocking(true)?;

    // Bind and listen
    socket.bind(&addr.into())?;
    socket.listen(1024)?; // Backlog of 1024 connections

    // Convert socket2::Socket to tokio::net::TcpListener
    let std_listener: std::net::TcpListener = socket.into();
    TcpListener::from_std(std_listener)
}

pub async fn handle_request(
    req: Request<Incoming>,
    state: Arc<AppState>,
    metrics: Arc<OptionalMetrics>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let start = std::time::Instant::now();
    metrics.record_request(req.method().as_str());

    // Check If-None-Match header
    if let Some(if_none_match) = req.headers().get("if-none-match")
        && if_none_match.as_bytes() == state.etag.as_bytes()
    {
        metrics.record_response(req.method().as_str(), 304, start);
        return Ok(Response::builder()
            .status(304)
            .body(Full::new(Bytes::new()))
            .unwrap());
    }

    // Check if client accepts gzip
    let use_compression = req
        .headers()
        .get("accept-encoding")
        .and_then(|val| val.to_str().ok())
        .is_some_and(|val| val.contains("gzip"));

    // Preallocate response builder with common headers
    let response = Response::builder()
        .header("Content-Type", "text/html")
        .header("Cache-Control", "public, max-age=3600, must-revalidate")
        .header("ETag", state.etag.as_bytes())
        .header(
            "Content-Length",
            if use_compression {
                state.compressed_content_length
            } else {
                state.uncompressed_content_length
            },
        )
        .header(
            "Content-Encoding",
            if use_compression { "gzip" } else { "identity" },
        )
        .body(Full::new(if use_compression {
            state.compressed_content.clone()
        } else {
            state.uncompressed_content.clone()
        }))
        .unwrap();

    metrics.record_response(req.method().as_str(), response.status().as_u16(), start);

    Ok(response)
}

pub async fn run_server(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let metrics = Arc::new(OptionalMetrics::new(!args.disable_metrics));

    // Start metrics server only if metrics are enabled
    if let Some(metrics_arc) = metrics.get_arc() {
        let metrics_addr: SocketAddr = format!("{}:{}", args.addr, args.metrics_port)
            .parse()
            .expect("Failed to parse metrics address");

        tokio::spawn(async move {
            if let Err(e) = run_metrics_server(metrics_arc, metrics_addr).await {
                error!("Metrics server error: {}", e);
            }
        });
        info!("Metrics enabled on port {}", args.metrics_port);
    } else {
        info!("Metrics disabled for maximum performance");
    }

    // Read the HTML file at startup
    let html_content = std::fs::read_to_string(&args.index_path).map_err(|e| {
        error!("Failed to read index file: {}", e);
        e
    })?;
    let state = Arc::new(AppState::new(html_content));

    // Calculate optimal buffer size using clamp
    let send_buffer_size =
        (state.uncompressed_content_length * 2).clamp(32 * 1024, 2 * 1024 * 1024); // Between 32KB and 2MB

    // Configure the server address
    let addr: SocketAddr = format!("{}:{}", args.addr, args.port)
        .parse()
        .expect("Failed to parse address");

    // Create optional connection limiter
    let conn_limiter = if args.max_connections > 0 {
        info!("Connection limit: {}", args.max_connections);
        Some(Arc::new(Semaphore::new(args.max_connections)))
    } else {
        info!("Connection limit: unlimited");
        None
    };

    if args.tls {
        info!("Initializing TLS server...");
        run_tls_server(args, addr, state, metrics, conn_limiter, send_buffer_size).await
    } else {
        info!("Initializing plain server...");
        run_plain_server(args, addr, state, metrics, conn_limiter, send_buffer_size).await
    }
}

async fn run_tls_server(
    args: Args,
    addr: SocketAddr,
    state: Arc<AppState>,
    metrics: Arc<OptionalMetrics>,
    conn_limiter: Option<Arc<Semaphore>>,
    send_buffer_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let tls_config = crate::tls::TlsConfig::new()?.into_server_config();
    let acceptor = TlsAcceptor::from(tls_config);
    let listener = create_listener(addr, args.reuse_port, send_buffer_size).await?;

    info!(
        "Server running on {}://{}",
        if args.tls { "https" } else { "http" },
        addr
    );

    // Create a graceful shutdown channel
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn shutdown signal handler
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (tcp_stream, _remote_addr) = match result {
                    Ok(conn) => conn,
                    Err(e) => {
                        error!("Failed to accept connection: {}", e);
                        continue;
                    }
                };

                // Optionally acquire connection permit
                let _permit = if let Some(ref limiter) = conn_limiter {
                    match limiter.clone().try_acquire_owned() {
                        Ok(p) => Some(p),
                        Err(_) => {
                            warn!("Connection limit reached, rejecting connection");
                            continue;
                        }
                    }
                } else {
                    None
                };

                let acceptor = acceptor.clone();
                let state = state.clone();
                let metrics = metrics.clone();

                tokio::spawn(async move {
                    // Perform TLS handshake
                    let tls_stream = match acceptor.accept(tcp_stream).await {
                        Ok(s) => s,
                        Err(e) => {
                            error!("TLS handshake failed: {}", e);
                            return;
                        }
                    };

                    let io = TokioIo::new(tls_stream);

                    let service = service_fn(move |req| {
                        let state = state.clone();
                        let metrics = metrics.clone();
                        async move { handle_request(req, state, metrics).await }
                    });

                    if let Err(e) = http1::Builder::new()
                        .keep_alive(true)
                        .serve_connection(io, service)
                        .await
                        && !e.is_incomplete_message()
                    {
                        error!("Connection error: {}", e);
                    }
                });
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("Shutting down TLS server...");
                    break;
                }
            }
        }
    }

    info!("Server shutdown complete");
    Ok(())
}

async fn run_plain_server(
    args: Args,
    addr: SocketAddr,
    state: Arc<AppState>,
    metrics: Arc<OptionalMetrics>,
    conn_limiter: Option<Arc<Semaphore>>,
    send_buffer_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = create_listener(addr, args.reuse_port, send_buffer_size).await?;

    info!(
        "Server running on {}://{}",
        if args.tls { "https" } else { "http" },
        addr
    );

    // Create a graceful shutdown channel
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn shutdown signal handler
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (tcp_stream, _remote_addr) = match result {
                    Ok(conn) => conn,
                    Err(e) => {
                        error!("Failed to accept connection: {}", e);
                        continue;
                    }
                };

                // Optionally acquire connection permit
                let _permit = if let Some(ref limiter) = conn_limiter {
                    match limiter.clone().try_acquire_owned() {
                        Ok(p) => Some(p),
                        Err(_) => {
                            warn!("Connection limit reached, rejecting connection");
                            continue;
                        }
                    }
                } else {
                    None
                };

                let state = state.clone();
                let metrics = metrics.clone();

                tokio::spawn(async move {
                    let io = TokioIo::new(tcp_stream);

                    let service = service_fn(move |req| {
                        let state = state.clone();
                        let metrics = metrics.clone();
                        async move { handle_request(req, state, metrics).await }
                    });

                    if let Err(e) = http1::Builder::new()
                        .keep_alive(true)
                        .serve_connection(io, service)
                        .await
                        && !e.is_incomplete_message()
                    {
                        error!("Connection error: {}", e);
                    }
                });
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("Shutting down server...");
                    break;
                }
            }
        }
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
