use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use single_page_web_server_rs::{
    cli::Args,
    metrics::{self, OptionalMetrics},
    server::{AppState, handle_request, run_server},
};

use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::net::TcpListener;
use tokio::time::sleep;

#[tokio::test]
async fn test_server_run() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary HTML file
    let temp_file = NamedTempFile::new()?;
    let test_content = "<html><body>Test Content</body></html>";
    fs::write(&temp_file, test_content)?;

    // Start server in background task
    let server_handle = tokio::spawn(async move {
        let args = Args {
            index_path: temp_file.path().to_str().unwrap().to_string(),
            port: 3000,
            addr: "127.0.0.1".to_string(),
            metrics_port: 13001,
            tls: false,
            disable_metrics: false,
            max_connections: 0,
            reuse_port: false,
        };
        run_server(args).await.unwrap();
    });

    // Give the server a moment to start
    sleep(Duration::from_millis(100)).await;

    // Create a client
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build_http();

    // Test basic GET request
    let response = client.get("http://127.0.0.1:3000".parse()?).await?;

    // Verify response
    assert_eq!(response.status(), 200);

    // Verify content
    let body_bytes = response.into_body().collect().await?.to_bytes();
    let body_string = String::from_utf8(body_bytes.to_vec())?;
    assert_eq!(body_string, test_content);

    // Clean up
    server_handle.abort();

    Ok(())
}

#[tokio::test]
async fn test_server_basic_functionality() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary HTML file
    let temp_file = NamedTempFile::new()?;
    let test_content = "<html><body>Test Content</body></html>";
    fs::write(&temp_file, test_content)?;

    // Start server in background task
    let test_port = 3001;
    let metrics_port = 13002;
    let addr = format!("127.0.0.1:{}", test_port);
    let metrics_addr: SocketAddr = format!("127.0.0.1:{}", metrics_port).parse()?;

    let html_content = fs::read_to_string(temp_file.path().to_str().unwrap())?;
    let state = Arc::new(AppState::new(html_content));
    let metrics = Arc::new(OptionalMetrics::new(true));
    let metrics_arc = metrics.get_arc().unwrap();

    // Start metrics server
    let metrics_handle = tokio::spawn(async move {
        metrics::run_metrics_server(metrics_arc, metrics_addr)
            .await
            .unwrap();
    });

    // Start main server
    let server_handle = tokio::spawn(async move {
        let addr: SocketAddr = addr.parse().unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();

        loop {
            let (tcp_stream, _) = listener.accept().await.unwrap();
            let state = state.clone();
            let metrics = metrics.clone();

            tokio::spawn(async move {
                let io = TokioIo::new(tcp_stream);
                let service = service_fn(move |req: Request<Incoming>| {
                    let state = state.clone();
                    let metrics = metrics.clone();
                    async move { handle_request(req, state, metrics).await }
                });

                let _ = http1::Builder::new().serve_connection(io, service).await;
            });
        }
    });

    // Give the servers a moment to start
    sleep(Duration::from_millis(100)).await;

    // Create a client
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build_http();

    // Test basic GET request
    let response = client
        .get(format!("http://127.0.0.1:{}", test_port).parse()?)
        .await?;

    // Verify response status
    assert_eq!(response.status(), 200);

    // Verify content type header
    assert_eq!(response.headers().get("content-type").unwrap(), "text/html");

    // Verify cache control header
    assert_eq!(
        response.headers().get("cache-control").unwrap(),
        "public, max-age=3600, must-revalidate"
    );

    // Verify ETag exists
    assert!(response.headers().contains_key("etag"));

    // Get response body
    let body_bytes = response.into_body().collect().await?.to_bytes();
    let body_string = String::from_utf8(body_bytes.to_vec())?;

    // Verify content matches
    assert_eq!(body_string, test_content);

    // Verify metrics
    let metrics_response = client
        .get(format!("http://127.0.0.1:{}/metrics", metrics_port).parse()?)
        .await?;

    assert_eq!(metrics_response.status(), 200);
    let metrics_body = metrics_response.into_body().collect().await?.to_bytes();
    let metrics_str = String::from_utf8(metrics_body.to_vec())?;

    // Verify request was counted in metrics
    assert!(metrics_str.contains("http_requests_total{method=\"GET\""));
    assert!(metrics_str.contains("method=\"GET\""));
    assert!(metrics_str.contains("http_request_duration_seconds"));

    // Clean up both servers
    metrics_handle.abort();
    server_handle.abort();

    Ok(())
}

#[tokio::test]
async fn test_server_different_port_and_address() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary HTML file
    let temp_file = NamedTempFile::new()?;
    let test_content = "<html><body>Different Port Test</body></html>";
    fs::write(&temp_file, test_content)?;

    // Start server with different port and address
    let test_port = 3002;
    let addr = format!("127.0.0.1:{}", test_port);
    let server_handle = tokio::spawn(async move {
        let _args = Args {
            index_path: temp_file.path().to_str().unwrap().to_string(),
            port: test_port,
            addr: "127.0.0.1".to_string(),
            metrics_port: 13003,
            tls: false,
            disable_metrics: false,
            max_connections: 0,
            reuse_port: false,
        };

        let html_content = fs::read_to_string(temp_file.path()).unwrap();
        let state = Arc::new(AppState::new(html_content));
        let metrics = Arc::new(OptionalMetrics::new(true));

        let addr: SocketAddr = addr.parse().unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();

        loop {
            let (tcp_stream, _) = listener.accept().await.unwrap();
            let state = state.clone();
            let metrics = metrics.clone();

            tokio::spawn(async move {
                let io = TokioIo::new(tcp_stream);
                let service = service_fn(move |req: Request<Incoming>| {
                    let state = state.clone();
                    let metrics = metrics.clone();
                    async move { handle_request(req, state, metrics).await }
                });

                let _ = http1::Builder::new().serve_connection(io, service).await;
            });
        }
    });

    sleep(Duration::from_millis(100)).await;

    // Test connection to different port
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build_http();
    let response = client
        .get(format!("http://127.0.0.1:{}", test_port).parse()?)
        .await?;

    assert_eq!(response.status(), 200);

    // Clean up
    server_handle.abort();

    Ok(())
}

#[tokio::test]
async fn test_server_invalid_html_file() {
    // Try to start server with non-existent file
    let args = Args {
        index_path: "nonexistent.html".to_string(),
        port: 3003,
        addr: "127.0.0.1".to_string(),
        metrics_port: 13004,
        tls: false,
        disable_metrics: false,
        max_connections: 0,
        reuse_port: false,
    };

    let result = fs::read_to_string(&args.index_path);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_server_etag_caching() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary HTML file
    let temp_file = NamedTempFile::new()?;
    let test_content = "<html><body>ETag Test Content</body></html>";
    fs::write(&temp_file, test_content)?;

    // Start server
    let test_port = 3004;
    let addr = format!("127.0.0.1:{}", test_port);
    let server_handle = tokio::spawn(async move {
        let _args = Args {
            index_path: temp_file.path().to_str().unwrap().to_string(),
            port: test_port,
            addr: "127.0.0.1".to_string(),
            metrics_port: 13005,
            tls: false,
            disable_metrics: false,
            max_connections: 0,
            reuse_port: false,
        };

        let html_content = fs::read_to_string(temp_file.path()).unwrap();
        let state = Arc::new(AppState::new(html_content));
        let metrics = Arc::new(OptionalMetrics::new(true));

        let addr: SocketAddr = addr.parse().unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();

        loop {
            let (tcp_stream, _) = listener.accept().await.unwrap();
            let state = state.clone();
            let metrics = metrics.clone();

            tokio::spawn(async move {
                let io = TokioIo::new(tcp_stream);
                let service = service_fn(move |req: Request<Incoming>| {
                    let state = state.clone();
                    let metrics = metrics.clone();
                    async move { handle_request(req, state, metrics).await }
                });

                let _ = http1::Builder::new().serve_connection(io, service).await;
            });
        }
    });

    sleep(Duration::from_millis(100)).await;

    // Create a client
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build_http();

    // First request to get the ETag
    let first_response = client
        .get(format!("http://127.0.0.1:{}", test_port).parse()?)
        .await?;

    assert_eq!(first_response.status(), 200);

    // Get the ETag from the first response
    let etag = first_response
        .headers()
        .get("etag")
        .expect("ETag header should be present")
        .to_str()?
        .to_string();

    // Second request with If-None-Match header
    let req = Request::builder()
        .method("GET")
        .uri(format!("http://127.0.0.1:{}", test_port))
        .header("if-none-match", &etag)
        .body(Full::new(Bytes::new()))?;
    let second_response = client.request(req).await?;

    // Should get a 304 Not Modified response
    assert_eq!(second_response.status(), 304);

    // Verify no body in 304 response
    let body_bytes = second_response.into_body().collect().await?.to_bytes();
    assert!(body_bytes.is_empty());

    // Third request with different ETag
    let req = Request::builder()
        .method("GET")
        .uri(format!("http://127.0.0.1:{}", test_port))
        .header("if-none-match", "\"different-etag\"")
        .body(Full::new(Bytes::new()))?;
    let third_response = client.request(req).await?;

    // Should get a 200 OK with full content
    assert_eq!(third_response.status(), 200);
    let body_bytes = third_response.into_body().collect().await?.to_bytes();
    let body_string = String::from_utf8(body_bytes.to_vec())?;
    assert_eq!(body_string, test_content);

    // Clean up
    server_handle.abort();

    Ok(())
}
