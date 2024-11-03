#[macro_use]
extern crate bencher;

use bencher::Bencher;

use hyper::Client;
use hyper::Server;
use hyper::service::{make_service_fn, service_fn};
use single_page_web_server_rs::{cli::Args, server::{AppState, handle_request}};
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::time::sleep;

fn bench_server_response_time(b: &mut Bencher) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    
    // Setup server (similar to other tests)
    let temp_file = runtime.block_on(async {
        let file = NamedTempFile::new().unwrap();
        fs::write(&file, "<html><body>Bench Test</body></html>").unwrap();
        file
    });

    let test_port = 3005;
    let addr = format!("127.0.0.1:{}", test_port);
    
    // Start server
    let server_handle = runtime.spawn(async move {
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

        Server::bind(&addr)
            .serve(make_svc)
            .await
            .unwrap();
    });

    // Give server time to start
    runtime.block_on(async {
        sleep(Duration::from_millis(100)).await;
    });

    // Create client
    let client = Client::new();
    let url: hyper::Uri = format!("http://127.0.0.1:{}", test_port).parse().unwrap();


    // Benchmark the request
    b.iter(|| {
        runtime.block_on(async {
            let response = client.get(url.clone()).await.unwrap();
            assert_eq!(response.status(), 200);
            let _body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        });
    });

    // Cleanup
    runtime.block_on(async {
        server_handle.abort();
    });
}

benchmark_group!(benches, bench_server_response_time);
benchmark_main!(benches);