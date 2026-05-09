//! Tests for Envoy-Atheneum Bridge HTTP endpoints
//! Tests are written FIRST (TDD) and will fail until implementation is complete.
//!
//! These tests only run when atheneum is available via dev-dependencies.

use http_body_util::BodyExt;
use std::sync::Arc;
use tower::util::ServiceExt;

use envoy::engine::Engine;
use envoy::http::AppState;
use serde_json::json;

// Helper to create test router with isolated temporary databases
// The TempDir must be kept alive for the test duration
fn setup_test_router() -> (axum::Router, tempfile::TempDir) {
    let db_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db_path = db_dir.path().join("test.db");
    let atheneum_path = db_dir.path().join("atheneum.db");
    let engine =
        Engine::open(db_path.to_str().expect("Invalid path")).expect("Failed to open engine");
    let state = Arc::new(
        AppState::with_atheneum_path(
            engine,
            atheneum_path.to_str().expect("Invalid atheneum path"),
        )
        .expect("Failed to create app state"),
    );
    let router = {
        #[cfg(feature = "atheneum")]
        {
            envoy::http::build_router_unlimited_with_atheneum(state)
        }
        #[cfg(not(feature = "atheneum"))]
        {
            compile_error!("atheneum feature must be enabled for this test")
        }
    };
    (router, db_dir)
}

// ============================================================================
// Discovery Endpoint Tests
// ============================================================================

#[tokio::test]
async fn test_post_discovery() {
    let (app, _temp_dir) = setup_test_router();

    let request_body = json!({
        "agent": "claude1",
        "discovery_type": "symbol",
        "target": "http_handler",
        "metadata": {
            "file_path": "src/http.rs",
            "line": 42,
            "kind": "function"
        }
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/atheneum/discoveries")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&request_body).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(response_json["discovery_id"].is_number());
    assert_eq!(response_json["agent"], "claude1");
    assert_eq!(response_json["target"], "http_handler");
}

#[tokio::test]
async fn test_get_discoveries() {
    let (app, _temp_dir) = setup_test_router();

    // First, store a discovery
    let store_request = json!({
        "agent": "claude1",
        "discovery_type": "symbol",
        "target": "http_handler",
        "metadata": {"file": "src/http.rs"}
    });

    let _ = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/atheneum/discoveries")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&store_request).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Query discoveries
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/atheneum/discoveries?target=http_handler")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(response_json["target"], "http_handler");
    assert_eq!(response_json["discovery_count"], 1);
    assert!(response_json["discoveries"].as_array().unwrap().len() > 0);
}

#[tokio::test]
async fn test_get_discoveries_empty() {
    let (app, _temp_dir) = setup_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/atheneum/discoveries?target=nonexistent")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(response_json["discovery_count"], 0);
    assert!(response_json["discoveries"].as_array().unwrap().is_empty());
}

// ============================================================================
// Handoff Endpoint Tests
// ============================================================================

#[tokio::test]
async fn test_post_handoff() {
    let (app, _temp_dir) = setup_test_router();

    let request_body = json!({
        "from_agent": "claude1",
        "to_agent": "claude2",
        "manifest": {
            "task": "implement auth",
            "files_analyzed": ["src/auth.rs"],
            "discoveries": 3
        }
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/atheneum/handoffs")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&request_body).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(response_json["handoff_id"].is_number());
    assert_eq!(response_json["from_agent"], "claude1");
    assert_eq!(response_json["to_agent"], "claude2");
}

#[tokio::test]
async fn test_get_pending_handoff() {
    let (app, _temp_dir) = setup_test_router();

    // Create a handoff
    let handoff_request = json!({
        "from_agent": "claude1",
        "to_agent": "claude2",
        "manifest": {"task": "fix bug"}
    });

    let _ = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/atheneum/handoffs")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&handoff_request).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Query pending handoff
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/atheneum/handoffs/pending?agent=claude2")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(response_json["handoff"].is_object());
    assert_eq!(response_json["handoff"]["from_agent"], "claude1");
    assert_eq!(response_json["handoff"]["to_agent"], "claude2");
}

#[tokio::test]
async fn test_get_pending_handoff_none() {
    let (app, _temp_dir) = setup_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/atheneum/handoffs/pending?agent=claude2")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(response_json["handoff"].is_null());
}

#[tokio::test]
async fn test_claim_handoff() {
    let (app, _temp_dir) = setup_test_router();

    // Create a handoff first
    let handoff_request = json!({
        "from_agent": "claude1",
        "to_agent": "claude2",
        "manifest": {"task": "test"}
    });

    let create_response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/atheneum/handoffs")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&handoff_request).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let create_body = BodyExt::collect(create_response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let create_json: serde_json::Value = serde_json::from_slice(&create_body).unwrap();
    let handoff_id = create_json["handoff_id"].as_i64().unwrap();

    // Claim the handoff
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri(&format!("/atheneum/handoffs/{}/claim", handoff_id))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Verify it's no longer pending
    let pending_response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/atheneum/handoffs/pending?agent=claude2")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let pending_body = BodyExt::collect(pending_response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let pending_json: serde_json::Value = serde_json::from_slice(&pending_body).unwrap();

    assert!(pending_json["handoff"].is_null());
}

// ============================================================================
// Knowledge Query Endpoint Tests
// ============================================================================

#[tokio::test]
async fn test_get_knowledge() {
    let (app, _temp_dir) = setup_test_router();

    // Store some discoveries
    let discovery1 = json!({
        "agent": "claude1",
        "discovery_type": "symbol",
        "target": "http_handler",
        "metadata": {"file": "src/http.rs"}
    });

    let discovery2 = json!({
        "agent": "claude2",
        "discovery_type": "cfg",
        "target": "http_handler",
        "metadata": {"complexity": 8}
    });

    let _ = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/atheneum/discoveries")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&discovery1).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let _ = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/atheneum/discoveries")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&discovery2).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Query knowledge
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/atheneum/knowledge?target=http_handler")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(response_json["target"], "http_handler");
    assert_eq!(response_json["discovery_count"], 2);
    assert!(response_json["token_savings"].is_object());
}

#[tokio::test]
async fn test_get_knowledge_empty() {
    let (app, _temp_dir) = setup_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/atheneum/knowledge?target=unknown")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(response_json["target"], "unknown");
    assert_eq!(response_json["discovery_count"], 0);
}

// ============================================================================
// Test Isolation Tests
// ============================================================================

#[tokio::test]
async fn test_isolation_separate_routers_no_data_leak() {
    // Two completely separate router instances should NOT share data
    let (app1, _temp_dir1) = setup_test_router();
    let (app2, _temp_dir2) = setup_test_router();

    // Store a discovery in app1
    let discovery = json!({
        "agent": "claude1",
        "discovery_type": "symbol",
        "target": "isolation_test_target",
        "metadata": {"test": "data"}
    });

    let _ = app1
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/atheneum/discoveries")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&discovery).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Query app2 — should NOT see the data from app1
    let response = app2
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/atheneum/discoveries?target=isolation_test_target")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // This assertion will FAIL with current implementation due to data leakage
    assert_eq!(
        response_json["discovery_count"], 0,
        "Separate router instances should NOT share atheneum data"
    );
}
