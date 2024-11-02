use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpSocket;
use clap::Parser;

// Cache the HTML content in memory for maximum performance
struct AppState {
    html_content: Arc<String>,
}

async fn handle_request(_req: Request<Body>, state: Arc<AppState>) -> Result<Response<Body>, Infallible> {
    Ok(Response::builder()
        .header("Content-Type", "text/html")
        // Cache for 1 year (31536000 seconds)
        .header("Cache-Control", "public, max-age=31536000, immutable")
        // Provide ETag for validation (using a simple hash of content)
        .header("ETag", format!("\"{}\"", 
            format!("{:x}", md5::compute(state.html_content.as_bytes())).chars().take(8).collect::<String>()))
        // Expires header as backup for HTTP/1.0 clients
        .header("Expires", httpdate::fmt_http_date(std::time::SystemTime::now() + 
            std::time::Duration::from_secs(31536000)))
        .body(Body::from(state.html_content.as_bytes().to_vec()))
        .unwrap())
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the index HTML file
    #[arg(long, default_value = "index.html", env="INDEX_PATH")]
    index_path: String,

    /// Port to listen on
    #[arg(long, default_value_t = 3000, env="LISTEN_PORT")]
    port: u16,

    /// Address to bind to
    #[arg(long, default_value = "127.0.0.1", env="LISTEN_ADDR")]
    addr: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args = Args::parse();

    // Read the HTML file at startup and keep it in memory
    let html_content = Arc::new(fs::read_to_string(&args.index_path)?);
    let state = Arc::new(AppState { html_content });

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

    // Run the server
    server.await?;

    Ok(())
}
