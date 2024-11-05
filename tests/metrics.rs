use std::sync::Arc;

use single_page_web_server_rs::metrics::Metrics;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use std::thread;

    #[test]
    fn test_metrics_recording() {
        let metrics = Arc::new(Metrics::new());
        
        // Simulate multiple requests
        for _ in 0..5 {
            metrics.record_request("GET");
            thread::sleep(Duration::from_millis(10)); // Simulate some work
            metrics.record_response("GET", 200, std::time::Instant::now());
        }

        for _ in 0..3 {
            metrics.record_request("POST");
            thread::sleep(Duration::from_millis(10));
            metrics.record_response("POST", 404, std::time::Instant::now());
        }

        // Gather Prometheus metrics
        let metric_families = metrics.get_metrics();
        
        // Helper function to find metric by name
        let find_metric = |name: &str| {
            metric_families.iter()
                .find(|m| m.get_name() == name)
                .expect(&format!("Metric {} not found", name))
        };

        // Verify request counts
        let requests_total = find_metric("http_requests_total");
        let get_requests = requests_total.get_metric().iter()
            .find(|m| m.get_label().iter().any(|l| l.get_value() == "GET"))
            .expect("GET requests not found");
        let post_requests = requests_total.get_metric().iter()
            .find(|m| m.get_label().iter().any(|l| l.get_value() == "POST"))
            .expect("POST requests not found");
        
        assert_eq!(get_requests.get_counter().get_value() as i64, 5);
        assert_eq!(post_requests.get_counter().get_value() as i64, 3);

        // Verify in-flight requests (should be 0 after all requests completed)
        let requests_in_flight = find_metric("http_requests_in_flight");
        for metric in requests_in_flight.get_metric() {
            assert_eq!(metric.get_gauge().get_value() as i64, 0);
        }

        // Verify duration histogram
        let duration = find_metric("http_request_duration_seconds");
        let get_200_duration = duration.get_metric().iter()
            .find(|m| m.get_label().iter().any(|l| l.get_value() == "GET") && 
                     m.get_label().iter().any(|l| l.get_value() == "200"))
            .expect("GET 200 duration not found");
        let post_404_duration = duration.get_metric().iter()
            .find(|m| m.get_label().iter().any(|l| l.get_value() == "POST") && 
                     m.get_label().iter().any(|l| l.get_value() == "404"))
            .expect("POST 404 duration not found");

        assert!(get_200_duration.get_histogram().get_sample_count() == 5);
        assert!(post_404_duration.get_histogram().get_sample_count() == 3);
    }

    #[test]
    fn test_concurrent_requests() {
        use std::thread;
        
        let metrics = Arc::new(Metrics::new());
        let mut handles = vec![];

        // Spawn 10 threads making concurrent requests
        for i in 0..10 {
            let metrics = metrics.clone();
            let handle = thread::spawn(move || {
                let method = if i % 2 == 0 { "GET" } else { "POST" };
                metrics.record_request(method);
                thread::sleep(Duration::from_millis(5));
                metrics.record_response(method, 200, std::time::Instant::now());
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify total request count
        let metric_families = metrics.get_metrics();
        let requests_total = metric_families.iter()
            .find(|m| m.get_name() == "http_requests_total")
            .unwrap();

        let total_requests: u64 = requests_total.get_metric().iter()
            .map(|m| m.get_counter().get_value() as u64)
            .sum();

        assert_eq!(total_requests, 10);
        
        // Verify no requests are in flight
        let requests_in_flight = metric_families.iter()
            .find(|m| m.get_name() == "http_requests_in_flight")
            .unwrap();

        let total_in_flight: i64 = requests_in_flight.get_metric().iter()
            .map(|m| m.get_gauge().get_value() as i64)
            .sum();

        assert_eq!(total_in_flight, 0);
    }
}

#[test]
fn test_metrics_iterator() {
    let metrics = Arc::new(Metrics::new());
    
    // Record some data
    metrics.record_request("GET");
    metrics.record_response("GET", 200, std::time::Instant::now());

    // Use the iterator
    let iter = metrics.metrics_iter();
    
    // Find specific metric
    let requests_total = iter.find_metric("http_requests_total")
        .expect("http_requests_total metric should exist");
    
    // Verify the metric
    let get_requests = requests_total.get_metric().iter()
        .find(|m| m.get_label().iter().any(|l| l.get_value() == "GET"))
        .expect("GET requests not found");
    
    assert_eq!(get_requests.get_counter().get_value() as i64, 1);
}