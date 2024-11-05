use hyper::{Body, Request, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use opentelemetry::{metrics::*, KeyValue};
use prometheus::{Encoder, TextEncoder};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, error};

pub use crate::server::shutdown_signal;

pub struct Metrics {
    requests_total: Counter<u64>,
    requests_in_flight: UpDownCounter<i64>,
    request_duration: Histogram<f64>,
    prom_requests_total: prometheus::IntCounterVec,
    prom_requests_in_flight: prometheus::IntGaugeVec,
    prom_request_duration: prometheus::HistogramVec,
    registry: prometheus::Registry,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        let meter = opentelemetry::global::meter("http_server");
        let registry = prometheus::Registry::new();
        
        // Create Prometheus metrics with labels
        let prom_requests_total = prometheus::IntCounterVec::new(
            prometheus::Opts::new("http_requests_total", "Total number of HTTP requests"),
            &["method"]
        ).unwrap();
    
        let prom_requests_in_flight = prometheus::IntGaugeVec::new(
            prometheus::Opts::new("http_requests_in_flight", "Number of HTTP requests currently in flight"),
            &["method"]
        ).unwrap();
    
        let prom_request_duration = prometheus::HistogramVec::new(
            prometheus::HistogramOpts::new(
                "http_request_duration_seconds",
                "HTTP request duration in seconds",
            ),
            &["method", "status"]
        ).unwrap();
        
        // Register metrics with Prometheus
        registry.register(Box::new(prom_requests_total.clone())).unwrap();
        registry.register(Box::new(prom_requests_in_flight.clone())).unwrap();
        registry.register(Box::new(prom_request_duration.clone())).unwrap();

        // Create OpenTelemetry metrics
        let otel_requests_total = meter
            .u64_counter("http_requests_total")
            .with_description("Total number of HTTP requests")
            .init();

        let otel_requests_in_flight = meter
            .i64_up_down_counter("http_requests_in_flight")
            .with_description("Number of HTTP requests currently in flight")
            .init();

        let otel_request_duration = meter
            .f64_histogram("http_request_duration_seconds")
            .with_description("HTTP request duration in seconds")
            .init();

        Self {
            requests_total: otel_requests_total,
            requests_in_flight: otel_requests_in_flight,
            request_duration: otel_request_duration,
            prom_requests_total,
            prom_requests_in_flight,
            prom_request_duration,
            registry,
        }
    }

    pub fn record_request(&self, method: &str) {
        let attributes = &[KeyValue::new("method", method.to_string())];
        self.requests_total.add(1, attributes);
        self.requests_in_flight.add(1, attributes);
        // Update Prometheus metrics with labels
        self.prom_requests_total.with_label_values(&[method]).inc();
        self.prom_requests_in_flight.with_label_values(&[method]).inc();
    }

    pub fn record_response(&self, method: &str, status: u16, start: std::time::Instant) {
        let attributes = &[
            KeyValue::new("method", method.to_string()),
            KeyValue::new("status", status.to_string()),
        ];
        let duration = start.elapsed().as_secs_f64();
        self.request_duration.record(duration, attributes);
        self.requests_in_flight.add(-1, attributes);
        // Update Prometheus metrics with labels
        self.prom_request_duration.with_label_values(&[method, &status.to_string()]).observe(duration);
        self.prom_requests_in_flight.with_label_values(&[method]).dec();
    }

    /// Returns a vector of metric families from the Prometheus registry
    pub fn get_metrics(&self) -> Vec<prometheus::proto::MetricFamily> {
        self.registry.gather()
    }

    /// Returns an iterator over metrics with helper methods to find specific metrics
    pub fn metrics_iter(&self) -> MetricsIterator {
        MetricsIterator {
            metrics: self.get_metrics()
        }
    }
}

/// Helper struct to iterate and find metrics easily
pub struct MetricsIterator {
    metrics: Vec<prometheus::proto::MetricFamily>
}

impl MetricsIterator {
    /// Find a metric by name
    pub fn find_metric(&self, name: &str) -> Option<&prometheus::proto::MetricFamily> {
        self.metrics.iter().find(|m| m.get_name() == name)
    }

    /// Get all metrics
    pub fn all(&self) -> &[prometheus::proto::MetricFamily] {
        &self.metrics
    }
}

async fn metrics_handler(metrics: Arc<Metrics>) -> std::result::Result<Response<Body>, Infallible> {
    let encoder = TextEncoder::new();
    let metric_families = metrics.registry.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();

    Ok(Response::builder()
        .header("Content-Type", "text/plain")
        .body(Body::from(buffer))
        .unwrap())
}

pub async fn run_metrics_server(metrics: Arc<Metrics>, addr: SocketAddr) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Create the service
    let make_svc = make_service_fn(move |_conn| {
        let metrics = metrics.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |_req: Request<Body>| {
                metrics_handler(metrics.clone())
            }))
        }
    });

    // Create and configure the server
    let server = Server::bind(&addr)
        .http1_keepalive(true)
        .tcp_nodelay(true)
        .serve(make_svc);

    info!("Metrics server running on http://{}/metrics", addr);

    // Handle graceful shutdown
    let graceful = server.with_graceful_shutdown(shutdown_signal());

    // Run the server
    if let Err(e) = graceful.await {
        error!("Server error: {}", e);
        return Err(e.into());
    }

    info!("Metrics server shutdown complete");
    Ok(())
} 