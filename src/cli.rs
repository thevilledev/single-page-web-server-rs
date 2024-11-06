use clap::Parser;

#[derive(Parser, Debug)]
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

    /// Metrics server port
    #[arg(long, default_value = "3001", env="METRICS_PORT")]
    pub metrics_port: u16,

    /// Enable TLS with self-signed certificate
    #[arg(long, default_value= "false", env="ENABLE_TLS")]
    pub tls: bool,
}