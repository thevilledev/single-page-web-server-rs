use clap::Parser;

use tracing::{info, error};

use single_page_web_server_rs::{cli::Args, server::run_server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args = Args::parse();
    info!("Starting server with configuration: {:?}", args);

    // Run the server
    if let Err(e) = run_server(args).await {
        error!("Server error: {}", e);
        return Err(e);
    }

    info!("Server shutdown complete");
    Ok(())
}
