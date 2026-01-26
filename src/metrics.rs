use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use opentelemetry::{KeyValue, metrics::*};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::{Encoder, Registry};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock};
use tokio::net::TcpListener;
use tracing::{error, info};

pub use crate::server::shutdown_signal;

// Pre-allocated KeyValue arrays for common HTTP methods to avoid per-request allocations
static METHOD_GET: LazyLock<[KeyValue; 1]> = LazyLock::new(|| [KeyValue::new("method", "GET")]);
static METHOD_POST: LazyLock<[KeyValue; 1]> = LazyLock::new(|| [KeyValue::new("method", "POST")]);
static METHOD_HEAD: LazyLock<[KeyValue; 1]> = LazyLock::new(|| [KeyValue::new("method", "HEAD")]);
static METHOD_PUT: LazyLock<[KeyValue; 1]> = LazyLock::new(|| [KeyValue::new("method", "PUT")]);
static METHOD_DELETE: LazyLock<[KeyValue; 1]> =
    LazyLock::new(|| [KeyValue::new("method", "DELETE")]);
static METHOD_OPTIONS: LazyLock<[KeyValue; 1]> =
    LazyLock::new(|| [KeyValue::new("method", "OPTIONS")]);

#[inline]
fn get_method_attributes(method: &str) -> &'static [KeyValue] {
    match method {
        "GET" => &*METHOD_GET,
        "POST" => &*METHOD_POST,
        "HEAD" => &*METHOD_HEAD,
        "PUT" => &*METHOD_PUT,
        "DELETE" => &*METHOD_DELETE,
        "OPTIONS" => &*METHOD_OPTIONS,
        _ => &*METHOD_GET, // Fallback to GET for unknown methods (rare case)
    }
}

// Pre-allocated KeyValue arrays for common status codes
static STATUS_200_GET: LazyLock<[KeyValue; 2]> = LazyLock::new(|| {
    [
        KeyValue::new("method", "GET"),
        KeyValue::new("status", "200"),
    ]
});
static STATUS_200_POST: LazyLock<[KeyValue; 2]> = LazyLock::new(|| {
    [
        KeyValue::new("method", "POST"),
        KeyValue::new("status", "200"),
    ]
});
static STATUS_200_HEAD: LazyLock<[KeyValue; 2]> = LazyLock::new(|| {
    [
        KeyValue::new("method", "HEAD"),
        KeyValue::new("status", "200"),
    ]
});
static STATUS_304_GET: LazyLock<[KeyValue; 2]> = LazyLock::new(|| {
    [
        KeyValue::new("method", "GET"),
        KeyValue::new("status", "304"),
    ]
});
static STATUS_304_POST: LazyLock<[KeyValue; 2]> = LazyLock::new(|| {
    [
        KeyValue::new("method", "POST"),
        KeyValue::new("status", "304"),
    ]
});
static STATUS_304_HEAD: LazyLock<[KeyValue; 2]> = LazyLock::new(|| {
    [
        KeyValue::new("method", "HEAD"),
        KeyValue::new("status", "304"),
    ]
});
static STATUS_404_GET: LazyLock<[KeyValue; 2]> = LazyLock::new(|| {
    [
        KeyValue::new("method", "GET"),
        KeyValue::new("status", "404"),
    ]
});
static STATUS_500_GET: LazyLock<[KeyValue; 2]> = LazyLock::new(|| {
    [
        KeyValue::new("method", "GET"),
        KeyValue::new("status", "500"),
    ]
});

#[inline]
fn get_response_attributes(method: &str, status: u16) -> Option<&'static [KeyValue]> {
    match (method, status) {
        ("GET", 200) => Some(&*STATUS_200_GET),
        ("POST", 200) => Some(&*STATUS_200_POST),
        ("HEAD", 200) => Some(&*STATUS_200_HEAD),
        ("GET", 304) => Some(&*STATUS_304_GET),
        ("POST", 304) => Some(&*STATUS_304_POST),
        ("HEAD", 304) => Some(&*STATUS_304_HEAD),
        ("GET", 404) => Some(&*STATUS_404_GET),
        ("GET", 500) => Some(&*STATUS_500_GET),
        _ => None, // Fallback to dynamic allocation for uncommon combinations
    }
}

pub struct Metrics {
    requests_total: Counter<u64>,
    requests_in_flight: UpDownCounter<i64>,
    request_duration: Histogram<f64>,
    registry: Registry,
    _provider: SdkMeterProvider,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper for optional metrics - allows zero-cost metrics when disabled
pub enum OptionalMetrics {
    Enabled(Arc<Metrics>),
    Disabled,
}

impl OptionalMetrics {
    pub fn new(enabled: bool) -> Self {
        if enabled {
            OptionalMetrics::Enabled(Arc::new(Metrics::new()))
        } else {
            OptionalMetrics::Disabled
        }
    }

    #[inline]
    pub fn record_request(&self, method: &str) {
        if let OptionalMetrics::Enabled(metrics) = self {
            metrics.record_request(method);
        }
    }

    #[inline]
    pub fn record_response(&self, method: &str, status: u16, start: std::time::Instant) {
        if let OptionalMetrics::Enabled(metrics) = self {
            metrics.record_response(method, status, start);
        }
    }

    pub fn is_enabled(&self) -> bool {
        matches!(self, OptionalMetrics::Enabled(_))
    }

    /// Get a clone of the Arc<Metrics> for use with the metrics server
    pub fn get_arc(&self) -> Option<Arc<Metrics>> {
        match self {
            OptionalMetrics::Enabled(metrics) => Some(metrics.clone()),
            OptionalMetrics::Disabled => None,
        }
    }
}

impl Metrics {
    pub fn new() -> Self {
        // Create a custom registry
        let registry = Registry::new();

        // Create a new prometheus exporter with the custom registry
        let exporter = opentelemetry_prometheus::exporter()
            .with_registry(registry.clone())
            .build()
            .unwrap();

        // Create a new meter provider using a reference to the exporter
        let provider = SdkMeterProvider::builder().with_reader(exporter).build();

        // Create a meter from the provider
        let meter = provider.meter("single_web_page_server_rs");

        let requests_total = meter
            .u64_counter("http_requests")
            .with_description("Total number of HTTP requests")
            .init();

        let requests_in_flight = meter
            .i64_up_down_counter("http_requests_in_flight")
            .with_description("Number of HTTP requests currently in flight")
            .init();

        let request_duration = meter
            .f64_histogram("http_request_duration_seconds")
            .with_description("HTTP request duration in seconds")
            .init();

        Self {
            requests_total,
            requests_in_flight,
            request_duration,
            registry,
            _provider: provider,
        }
    }

    #[inline]
    pub fn record_request(&self, method: &str) {
        let attributes = get_method_attributes(method);
        self.requests_total.add(1, attributes);
        self.requests_in_flight.add(1, attributes);
    }

    #[inline]
    pub fn record_response(&self, method: &str, status: u16, start: std::time::Instant) {
        let duration = start.elapsed().as_secs_f64();

        // Use pre-allocated attributes for common method/status combinations
        if let Some(attributes) = get_response_attributes(method, status) {
            self.request_duration.record(duration, attributes);
        } else {
            // Fallback to dynamic allocation for uncommon combinations
            let attributes = &[
                KeyValue::new("method", method.to_string()),
                KeyValue::new("status", status.to_string()),
            ];
            self.request_duration.record(duration, attributes);
        }

        // Use pre-allocated method attributes for in-flight counter
        let method_attrs = get_method_attributes(method);
        self.requests_in_flight.add(-1, method_attrs);
    }

    pub fn get_metrics(&self) -> Vec<prometheus::proto::MetricFamily> {
        self.registry.gather()
    }

    pub fn collect_metrics(&self) {
        // Force a collection of metrics
        _ = self._provider.force_flush();
    }
}

async fn metrics_handler(
    req: Request<Incoming>,
    metrics: Arc<Metrics>,
) -> std::result::Result<Response<Full<Bytes>>, Infallible> {
    match req.uri().path() {
        "/metrics" => {
            let metric_families = metrics.get_metrics();
            let mut buffer = Vec::new();
            let encoder = prometheus::TextEncoder::new();
            encoder.encode(&metric_families, &mut buffer).unwrap();

            Ok(Response::builder()
                .header("Content-Type", "text/plain")
                .body(Full::new(Bytes::from(buffer)))
                .unwrap())
        }
        _ => Ok(Response::builder()
            .status(404)
            .body(Full::new(Bytes::from("Not Found")))
            .unwrap()),
    }
}

pub async fn run_metrics_server(
    metrics: Arc<Metrics>,
    addr: SocketAddr,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(addr).await?;

    info!("Metrics server running on http://{}/metrics", addr);

    // Create a graceful shutdown channel
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn shutdown signal handler
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (tcp_stream, _) = match result {
                    Ok(conn) => conn,
                    Err(e) => {
                        error!("Failed to accept metrics connection: {}", e);
                        continue;
                    }
                };

                let metrics = metrics.clone();

                tokio::spawn(async move {
                    let io = TokioIo::new(tcp_stream);

                    let service = service_fn(move |req| {
                        let metrics = metrics.clone();
                        async move { metrics_handler(req, metrics).await }
                    });

                    if let Err(e) = http1::Builder::new()
                        .keep_alive(true)
                        .serve_connection(io, service)
                        .await
                        && !e.is_incomplete_message()
                    {
                        error!("Metrics connection error: {}", e);
                    }
                });
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("Shutting down metrics server...");
                    break;
                }
            }
        }
    }

    info!("Metrics server shutdown complete");
    Ok(())
}
