use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpSocket;
use clap::Parser;
use tokio::signal;
use tracing::{info, error};

use single_page_web_server_rs::{cli::Args, server::{AppState, handle_request}};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args = Args::parse();
    info!("Starting server with configuration: {:?}", args);

    // Read the HTML file at startup and keep it in memory
    let html_content = fs::read_to_string(&args.index_path)
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

    println!("Server running on http://{}", addr);

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
