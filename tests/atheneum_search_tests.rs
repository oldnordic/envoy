//! Tests for HTTP-level semantic search via /atheneum/search.

#![cfg(feature = "atheneum")]

mod atheneum_bridge_module;

use http_body_util::BodyExt;
use std::sync::Arc;
use tower::util::ServiceExt;

use envoy::engine::Engine;
use serde_json::json;

use atheneum_bridge_module::TestState;

fn setup_test_router() -> (axum::Router, tempfile::TempDir) {
    let db_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db_path = db_dir.path().join("test.db");
    let atheneum_path = db_dir.path().join("atheneum.db");
    let engine =
        Engine::open(db_path.to_str().expect("Invalid path")).expect("Failed to open engine");
    let state = Arc::new(TestState {
        engine: Arc::new(std::sync::Mutex::new(engine)),
        atheneum_path: atheneum_path
            .to_str()
            .expect("Invalid atheneum path")
            .to_string(),
    });
    let router = atheneum_bridge_module::build_test_router(state);
    (router, db_dir)
}

async fn post_json(
    app: &axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> axum::http::StatusCode {
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri(uri)
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&body).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status()
}

async fn get_json(app: &axum::Router, uri: &str) -> (axum::http::StatusCode, serde_json::Value) {
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(json!(null));
    (status, value)
}

#[tokio::test]
async fn test_search_returns_matching_discovery() {
    let (app, _td) = setup_test_router();

    assert_eq!(
        post_json(
            &app,
            "/atheneum/discoveries",
            json!({
                "agent": "a",
                "discovery_type": "Symbol",
                "target": "build_router",
                "metadata": {"summary": "constructs the axum router with routes"}
            }),
        )
        .await,
        axum::http::StatusCode::CREATED
    );
    assert_eq!(
        post_json(
            &app,
            "/atheneum/discoveries",
            json!({
                "agent": "a",
                "discovery_type": "Symbol",
                "target": "parse_yaml",
                "metadata": {"summary": "parses YAML frontmatter from markdown"}
            }),
        )
        .await,
        axum::http::StatusCode::CREATED
    );

    let (s, body) = get_json(&app, "/atheneum/search?q=router%20axum%20routes&k=3").await;
    assert_eq!(s, axum::http::StatusCode::OK);
    let results = body["results"].as_array().expect("results array");
    assert!(!results.is_empty(), "search should return matches");
    assert_eq!(
        results[0]["name"],
        json!("a: build_router"),
        "best match should be the router discovery"
    );
}

#[tokio::test]
async fn test_search_respects_project_filter() {
    let (app, _td) = setup_test_router();

    post_json(
        &app,
        "/atheneum/discoveries",
        json!({
            "agent": "a", "discovery_type": "Symbol", "target": "Message",
            "project_id": "envoy",
            "metadata": {"summary": "envoy message struct"}
        }),
    )
    .await;
    post_json(
        &app,
        "/atheneum/discoveries",
        json!({
            "agent": "a", "discovery_type": "Symbol", "target": "Message",
            "project_id": "magellan",
            "metadata": {"summary": "magellan protocol message"}
        }),
    )
    .await;

    let (_, body) = get_json(&app, "/atheneum/search?q=message&k=10&project=envoy").await;
    let results = body["results"].as_array().expect("results array").clone();
    assert!(!results.is_empty(), "envoy filter should still match");
    for r in &results {
        assert_eq!(
            r["data"]["project_id"],
            json!("envoy"),
            "filter should reject non-envoy results"
        );
    }
}
