use clap::Parser;
use hyper::{Body, Request, Response};
use std::convert::Infallible;
use std::sync::Arc;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

pub use crate::server::{AppState, handle_request};
pub use crate::cli::Args;

fn compress_content(content: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(content.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

pub mod cli {
    use super::*;

    #[derive(Parser)]
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

    pub struct AppState {
        pub html_content: Arc<String>,
        pub etag: String,         // Pre-compute ETag
        pub content_length: usize, // Pre-compute content length
        pub compressed: Arc<Vec<u8>>, // Pre-compressed content
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
}