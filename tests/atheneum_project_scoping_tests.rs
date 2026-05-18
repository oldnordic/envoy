//! Tests for project/workspace scoping in the HTTP bridge.
//!
//! Stage 1 of the atheneum-py port: discoveries and handoffs carry an optional
//! project_id so envoy/magellan/splice can share one DB without name collisions.

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
) -> (axum::http::StatusCode, serde_json::Value) {
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri(uri)
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(json!(null));
    (status, value)
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
async fn test_discovery_filtered_by_project_param() {
    let (app, _td) = setup_test_router();

    // Two discoveries, same target, different projects
    let (s1, _) = post_json(
        &app,
        "/atheneum/discoveries",
        json!({
            "agent": "a1",
            "discovery_type": "Symbol",
            "target": "shared_name",
            "project_id": "envoy",
            "metadata": {"file": "envoy.rs"}
        }),
    )
    .await;
    assert_eq!(s1, axum::http::StatusCode::CREATED);

    let (s2, _) = post_json(
        &app,
        "/atheneum/discoveries",
        json!({
            "agent": "a2",
            "discovery_type": "Symbol",
            "target": "shared_name",
            "project_id": "magellan",
            "metadata": {"file": "magellan.rs"}
        }),
    )
    .await;
    assert_eq!(s2, axum::http::StatusCode::CREATED);

    let (s3, envoy_only) =
        get_json(&app, "/atheneum/discoveries?target=shared_name&project=envoy").await;
    assert_eq!(s3, axum::http::StatusCode::OK);
    assert_eq!(envoy_only["discovery_count"], json!(1));

    let (s4, magellan_only) = get_json(
        &app,
        "/atheneum/discoveries?target=shared_name&project=magellan",
    )
    .await;
    assert_eq!(s4, axum::http::StatusCode::OK);
    assert_eq!(magellan_only["discovery_count"], json!(1));

    let (s5, all) = get_json(&app, "/atheneum/discoveries?target=shared_name").await;
    assert_eq!(s5, axum::http::StatusCode::OK);
    assert_eq!(
        all["discovery_count"],
        json!(2),
        "no project filter should return both"
    );
}

#[tokio::test]
async fn test_pending_handoff_filtered_by_project_param() {
    let (app, _td) = setup_test_router();

    let (s1, _) = post_json(
        &app,
        "/atheneum/handoffs",
        json!({
            "from_agent": "alice",
            "to_agent": "bob",
            "project_id": "envoy",
            "manifest": {"status": "NEEDS_CONTEXT", "what_was_done": "envoy work"}
        }),
    )
    .await;
    assert_eq!(s1, axum::http::StatusCode::CREATED);

    let (s2, _) = post_json(
        &app,
        "/atheneum/handoffs",
        json!({
            "from_agent": "alice",
            "to_agent": "bob",
            "project_id": "magellan",
            "manifest": {"status": "NEEDS_CONTEXT", "what_was_done": "magellan work"}
        }),
    )
    .await;
    assert_eq!(s2, axum::http::StatusCode::CREATED);

    let (s3, envoy_pending) =
        get_json(&app, "/atheneum/handoffs/pending?agent=bob&project=envoy").await;
    assert_eq!(s3, axum::http::StatusCode::OK);
    assert_eq!(
        envoy_pending["handoff"]["manifest"]["what_was_done"],
        json!("envoy work")
    );

    let (s4, mag_pending) = get_json(
        &app,
        "/atheneum/handoffs/pending?agent=bob&project=magellan",
    )
    .await;
    assert_eq!(s4, axum::http::StatusCode::OK);
    assert_eq!(
        mag_pending["handoff"]["manifest"]["what_was_done"],
        json!("magellan work")
    );
}
