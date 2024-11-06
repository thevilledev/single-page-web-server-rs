use hyper::{Body, Request, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use opentelemetry::{metrics::*, KeyValue};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::{ Registry, Encoder};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, error};

pub use crate::server::shutdown_signal;

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
        let provider = SdkMeterProvider::builder()
            .with_reader(exporter)
            .build();

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

    pub fn record_request(&self, method: &str) {
        let attributes = &[KeyValue::new("method", method.to_string())];
        self.requests_total.add(1, attributes);
        self.requests_in_flight.add(1, attributes);
    }

    pub fn record_response(&self, method: &str, status: u16, start: std::time::Instant) {
        let attributes_duration = &[
            KeyValue::new("method", method.to_string()),
            KeyValue::new("status", status.to_string()),
        ];
        let attributes_in_flight = &[
            KeyValue::new("method", method.to_string()),
        ];
        let duration = start.elapsed().as_secs_f64();
        self.request_duration.record(duration, attributes_duration);
        self.requests_in_flight.add(-1, attributes_in_flight);
    }

    pub fn get_metrics(&self) -> Vec<prometheus::proto::MetricFamily> {
        return self.registry.gather();
    }

    pub fn collect_metrics(&self) {
        // Force a collection of metrics
        _ = self._provider.force_flush();
    }
}

async fn metrics_handler(req: Request<Body>, metrics: Arc<Metrics>) -> std::result::Result<Response<Body>, Infallible> {
    match req.uri().path() {
        "/metrics" => {
            let metric_families = metrics.get_metrics();
            let mut buffer = Vec::new();
            let encoder = prometheus::TextEncoder::new();
            encoder.encode(&metric_families, &mut buffer).unwrap();

            Ok(Response::builder()
                .header("Content-Type", "text/plain")
                .body(Body::from(buffer))
                .unwrap())
        }
        _ => Ok(Response::builder()
            .status(404)
            .body(Body::from("Not Found"))
            .unwrap())
    }
}

pub async fn run_metrics_server(metrics: Arc<Metrics>, addr: SocketAddr) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let make_svc = make_service_fn(move |_conn| {
        let metrics = metrics.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req: Request<Body>| {
                metrics_handler(req, metrics.clone())
            }))
        }
    });

    let server = Server::bind(&addr)
        .http1_keepalive(true)
        .tcp_nodelay(true)
        .serve(make_svc);

    info!("Metrics server running on http://{}/metrics", addr);

    let graceful = server.with_graceful_shutdown(shutdown_signal());

    if let Err(e) = graceful.await {
        error!("Server error: {}", e);
        return Err(e.into());
    }

    info!("Metrics server shutdown complete");
    Ok(())
}
