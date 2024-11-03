#[macro_use]
extern crate bencher;

use bencher::Bencher;

use hyper::Client;
use single_page_web_server_rs::{cli::Args, server::run_server};
use std::fs;
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
    
    // Start server
    let server_handle = runtime.spawn(async move {
        let args = Args {
            index_path: temp_file.path().to_str().unwrap().to_string(),
            port: test_port,
            addr: "127.0.0.1".to_string(),
        };
        
        run_server(args)
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