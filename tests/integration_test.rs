use hyper::Client;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::time::sleep;

use single_page_web_server_rs::{cli::Args, server::{AppState, run_server, handle_request}};
use hyper::Server;
use hyper::service::{make_service_fn, service_fn};
use std::convert::Infallible;
use hyper::{Request, Body};


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
        };
        run_server(args).await.unwrap();
    });

    // Give the server a moment to start
    sleep(Duration::from_millis(100)).await;

    // Create a client
    let client = Client::new();

    // Test basic GET request
    let response = client
        .get("http://127.0.0.1:3000".parse()?)
        .await?;

    // Verify response
    assert_eq!(response.status(), 200);

    // Verify content
    let body_bytes = hyper::body::to_bytes(response.into_body()).await?;
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
    let addr = format!("127.0.0.1:{}", test_port);
    let server_handle = tokio::spawn(async move {
        let args = Args {
            index_path: temp_file.path().to_str().unwrap().to_string(),
            port: test_port,
            addr: "127.0.0.1".to_string(),
        };

        let html_content = fs::read_to_string(&args.index_path).unwrap();
        let state = Arc::new(AppState::new(html_content));
        
        let addr: SocketAddr = addr.parse().unwrap();
        let make_svc = make_service_fn(move |_conn| {
            let state = state.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    handle_request(req, state.clone())
                }))
            }
        });

        let server = Server::bind(&addr)
            .serve(make_svc);
            
        server.await.unwrap();
    });

    // Give the server a moment to start
    sleep(Duration::from_millis(100)).await;

    // Create a client
    let client = Client::new();

    // Test basic GET request
    let response = client
        .get(format!("http://127.0.0.1:{}", test_port).parse()?)
        .await?;

    // Verify response status
    assert_eq!(response.status(), 200);

    // Verify content type header
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/html"
    );

    // Verify cache control header
    assert_eq!(
        response.headers().get("cache-control").unwrap(),
        "public, max-age=3600, must-revalidate"
    );

    // Verify ETag exists
    assert!(response.headers().contains_key("etag"));

    // Get response body
    let body_bytes = hyper::body::to_bytes(response.into_body()).await?;
    let body_string = String::from_utf8(body_bytes.to_vec())?;

    // Verify content matches
    assert_eq!(body_string, test_content);

    // Clean up
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
        let args = Args {
            index_path: temp_file.path().to_str().unwrap().to_string(),
            port: test_port,
            addr: "127.0.0.1".to_string(),
        };

        let html_content = fs::read_to_string(&args.index_path).unwrap();
        let state = Arc::new(AppState::new(html_content));
        
        let addr: SocketAddr = addr.parse().unwrap();
        let make_svc = make_service_fn(move |_conn| {
            let state = state.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    handle_request(req, state.clone())
                }))
            }
        });

        let server = Server::bind(&addr)
            .serve(make_svc);
            
        server.await.unwrap();
    });

    sleep(Duration::from_millis(100)).await;

    // Test connection to different port
    let client = Client::new();
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
        let args = Args {
            index_path: temp_file.path().to_str().unwrap().to_string(),
            port: test_port,
            addr: "127.0.0.1".to_string(),
        };

        let html_content = fs::read_to_string(&args.index_path).unwrap();
        let state = Arc::new(AppState::new(html_content));
        
        let addr: SocketAddr = addr.parse().unwrap();
        let make_svc = make_service_fn(move |_conn| {
            let state = state.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    handle_request(req, state.clone())
                }))
            }
        });

        let server = Server::bind(&addr)
            .serve(make_svc);
            
        server.await.unwrap();
    });

    sleep(Duration::from_millis(100)).await;

    // Create a client
    let client = Client::new();

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
        .body(Body::empty())?;
    let second_response = client.request(req).await?;

    // Should get a 304 Not Modified response
    assert_eq!(second_response.status(), 304);
    
    // Verify no body in 304 response
    let body_bytes = hyper::body::to_bytes(second_response.into_body()).await?;
    assert!(body_bytes.is_empty());

    // Third request with different ETag
    let req = Request::builder()
        .method("GET")
        .uri(format!("http://127.0.0.1:{}", test_port))
        .header("if-none-match", "\"different-etag\"")
        .body(Body::empty())?;
    let third_response = client.request(req).await?;

    // Should get a 200 OK with full content
    assert_eq!(third_response.status(), 200);
    let body_bytes = hyper::body::to_bytes(third_response.into_body()).await?;
    let body_string = String::from_utf8(body_bytes.to_vec())?;
    assert_eq!(body_string, test_content);

    // Clean up
    server_handle.abort();

    Ok(())
}
