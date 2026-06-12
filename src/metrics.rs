//! Prometheus metrics for envoy.
//!
//! Uses the `metrics` facade with `metrics-exporter-prometheus` for scraping.
//! Exposes a `/metrics` endpoint for Prometheus to scrape.

use std::sync::LazyLock;
use std::time::Instant;

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

/// Histogram bucket boundaries for request duration (ms).
///
/// Buckets: 0.5ms, 1ms, 5ms, 10ms, 50ms, 100ms, 500ms, 1s, 5s
const REQUEST_DURATION_BUCKETS: &[f64] = &[0.5, 1.0, 5.0, 10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0];

/// Global Prometheus handle. Initialized once at startup.
static HANDLE: LazyLock<PrometheusHandle> = LazyLock::new(|| {
    let builder = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("envoy_request_duration_ms".to_string()),
            REQUEST_DURATION_BUCKETS,
        )
        .expect("failed to set histogram buckets");
    let handle = builder
        .install_recorder()
        .expect("failed to install Prometheus recorder");
    describe_metrics();
    handle
});

/// Describe all metrics so they appear in `/metrics` output with documentation.
fn describe_metrics() {
    describe_counter!(
        "envoy_requests_total",
        "Total HTTP requests processed, labeled by operation and status class"
    );
    describe_histogram!(
        "envoy_request_duration_ms",
        "Request latency in milliseconds, labeled by operation"
    );
    describe_gauge!("envoy_agents_online", "Number of currently active agents");
    describe_gauge!(
        "envoy_messages_pending",
        "Number of undelivered messages in the store"
    );
    describe_gauge!(
        "envoy_ws_connections",
        "Number of active WebSocket connections"
    );
}

/// Initialize the Prometheus metrics recorder and return the handle.
///
/// Call once at server startup. The `LazyLock` ensures this is safe to call
/// multiple times — only the first call does real work.
pub fn init() -> &'static PrometheusHandle {
    &HANDLE
}

/// Render the Prometheus metrics text output.
pub fn render() -> String {
    HANDLE.render()
}

/// AXUM handler for `GET /metrics`.
pub async fn metrics_endpoint() -> impl IntoResponse {
    let body = render();
    (
        axum::http::StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}

/// Middleware that records request count and latency for every HTTP request.
pub async fn metrics_middleware(request: Request<Body>, next: Next) -> Response {
    let start = Instant::now();
    let method = request.method().clone();
    let path = normalize_path(request.uri().path());

    let response = next.run(request).await;

    let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
    let status = response.status().as_u16();

    record_request(&method, &path, status);
    record_latency(&path, latency_ms);

    response
}

/// Normalize a path into a stable label for metrics.
///
/// Collapses path segments that look like IDs (numeric or `id\d+` patterns)
/// into a single `:id` placeholder to avoid cardinality explosion.
fn normalize_path(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    let mut normalized = Vec::with_capacity(segments.len());
    for seg in &segments {
        if seg.is_empty() {
            continue;
        }
        if is_id_segment(seg) {
            normalized.push(":id");
        } else {
            normalized.push(seg);
        }
    }
    if normalized.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", normalized.join("/"))
    }
}

/// Check if a path segment looks like an ID (numeric, `id\d+`, or UUID).
fn is_id_segment(seg: &str) -> bool {
    if seg.parse::<u64>().is_ok() || seg.starts_with("id") {
        return true;
    }
    // UUID pattern: 8-4-4-4-12 hex chars
    let bytes = seg.as_bytes();
    if bytes.len() == 36 {
        let has_dashes =
            bytes[8] == b'-' && bytes[13] == b'-' && bytes[18] == b'-' && bytes[23] == b'-';
        if has_dashes {
            return seg.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        }
    }
    false
}

/// Record a request counter.
fn record_request(method: &axum::http::Method, path: &str, status: u16) {
    let status_class = match status {
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "other",
    };
    counter!(
        "envoy_requests_total",
        "method" => method.to_string(),
        "path" => path.to_string(),
        "status" => status_class.to_string()
    )
    .increment(1);
}

/// Record request latency.
fn record_latency(path: &str, latency_ms: f64) {
    histogram!("envoy_request_duration_ms", "path" => path.to_string()).record(latency_ms);
}

/// Update the agents-online gauge. Call after agent register/disconnect/retire.
pub fn set_agents_online(count: usize) {
    gauge!("envoy_agents_online").set(count as f64);
}

/// Update the pending-messages gauge. Call after send/poll/ack.
pub fn set_messages_pending(count: usize) {
    gauge!("envoy_messages_pending").set(count as f64);
}

/// Update the WebSocket connections gauge.
pub fn set_ws_connections(count: usize) {
    gauge!("envoy_ws_connections").set(count as f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_root() {
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn test_normalize_path_static() {
        assert_eq!(normalize_path("/agents"), "/agents");
        assert_eq!(normalize_path("/health"), "/health");
        assert_eq!(normalize_path("/atheneum/events"), "/atheneum/events");
    }

    #[test]
    fn test_normalize_path_numeric_id() {
        assert_eq!(normalize_path("/agents/42"), "/agents/:id");
        assert_eq!(normalize_path("/messages/123"), "/messages/:id");
    }

    #[test]
    fn test_normalize_path_named_id() {
        assert_eq!(normalize_path("/agents/id1118"), "/agents/:id");
        assert_eq!(normalize_path("/agents/id1.2"), "/agents/:id");
    }

    #[test]
    fn test_normalize_path_mixed() {
        assert_eq!(
            normalize_path("/agents/id5/messages/42/ack"),
            "/agents/:id/messages/:id/ack"
        );
    }

    #[test]
    fn test_is_id_segment() {
        assert!(is_id_segment("42"));
        assert!(is_id_segment("0"));
        assert!(is_id_segment("id1118"));
        assert!(is_id_segment("id1"));
        assert!(!is_id_segment("agents"));
        assert!(!is_id_segment("health"));
        assert!(!is_id_segment("ack"));
    }

    #[test]
    fn test_is_id_segment_uuid() {
        assert!(is_id_segment("338b8adc-6c08-4664-af1d-69300e7c576a"));
        assert!(is_id_segment("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!is_id_segment("not-a-uuid"));
        assert!(!is_id_segment("short"));
    }

    #[test]
    fn test_normalize_path_uuid() {
        assert_eq!(
            normalize_path("/atheneum/sessions/338b8adc-6c08-4664-af1d-69300e7c576a"),
            "/atheneum/sessions/:id"
        );
        assert_eq!(
            normalize_path("/atheneum/sessions/338b8adc-6c08-4664-af1d-69300e7c576a/handover"),
            "/atheneum/sessions/:id/handover"
        );
    }
}
