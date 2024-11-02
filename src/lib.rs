use clap::Parser;
use hyper::{Body, Request, Response};
use std::convert::Infallible;
use std::sync::Arc;

pub use crate::server::{AppState, handle_request};
pub use crate::cli::Args;

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
    }

    pub async fn handle_request(_req: Request<Body>, state: Arc<AppState>) -> Result<Response<Body>, Infallible> {
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
}