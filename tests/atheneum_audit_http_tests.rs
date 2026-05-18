//! Stage 9: HTTP tests for the audit-trail endpoints.
//!
//! POST /atheneum/actions writes the full provenance chain in one call,
//! GET /atheneum/actions?agent= walks it back. The chain itself is wired
//! by atheneum (Stage 6); this just proves the HTTP surface lines up
//! with the underlying methods and that project_id flows through.

#![cfg(feature = "atheneum")]

mod atheneum_bridge_module;

use http_body_util::BodyExt;
use std::sync::Arc;
use tower::util::ServiceExt;

use envoy::engine::Engine;
use serde_json::{json, Value};

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
    (atheneum_bridge_module::build_test_router(state), db_dir)
}

async fn req(
    app: &axum::Router,
    method: axum::http::Method,
    uri: &str,
    body: Option<Value>,
) -> (axum::http::StatusCode, Value) {
    let mut builder = axum::http::Request::builder().method(method).uri(uri);
    let req_body = match &body {
        Some(b) => {
            builder = builder.header(axum::http::header::CONTENT_TYPE, "application/json");
            axum::body::Body::from(serde_json::to_string(b).unwrap())
        }
        None => axum::body::Body::empty(),
    };
    let resp = app
        .clone()
        .oneshot(builder.body(req_body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(json!(null));
    (status, value)
}

#[tokio::test]
async fn test_post_action_writes_chain_and_returns_ids() {
    let (app, _td) = setup_test_router();

    // A pre-existing discovery to serve as the "modified" target
    let (_, disc) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/discoveries",
        Some(json!({
            "agent": "seed", "discovery_type": "Symbol", "target": "fn_a",
            "metadata": {}
        })),
    )
    .await;
    let target_id = disc["discovery_id"].as_i64().expect("discovery_id");

    let (status, body) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/actions",
        Some(json!({
            "agent": "claude1",
            "thought": "rename fn_a → fn_b",
            "project_id": "envoy",
            "tool_calls": [
                {
                    "tool_name": "splice_rename",
                    "args": {"from": "fn_a", "to": "fn_b"},
                    "modified_targets": [target_id]
                },
                {
                    "tool_name": "cargo_test",
                    "args": {},
                    "modified_targets": []
                }
            ]
        })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);

    assert!(body["reasoning_log_id"].as_i64().unwrap_or(0) > 0);
    assert!(body["agent_id"].as_i64().unwrap_or(0) > 0);
    assert_eq!(body["tool_call_ids"].as_array().map(|a| a.len()), Some(2));
    assert_eq!(
        body["modified_edge_ids"].as_array().map(|a| a.len()),
        Some(1),
        "one tool call modified one target → one Modified edge"
    );
}

#[tokio::test]
async fn test_get_actions_walks_chain_back() {
    let (app, _td) = setup_test_router();

    // Two actions for the same agent in the same project
    req(
        &app,
        axum::http::Method::POST,
        "/atheneum/actions",
        Some(json!({
            "agent": "a1",
            "thought": "first thought",
            "project_id": "envoy",
            "tool_calls": [
                {"tool_name": "edit", "args": {}, "modified_targets": []}
            ]
        })),
    )
    .await;
    req(
        &app,
        axum::http::Method::POST,
        "/atheneum/actions",
        Some(json!({
            "agent": "a1",
            "thought": "second thought",
            "project_id": "envoy",
            "tool_calls": []
        })),
    )
    .await;
    // A third action for a different project — must NOT show up
    req(
        &app,
        axum::http::Method::POST,
        "/atheneum/actions",
        Some(json!({
            "agent": "a1",
            "thought": "other project",
            "project_id": "magellan",
            "tool_calls": []
        })),
    )
    .await;

    let (s, body) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/actions?agent=a1&project=envoy",
        None,
    )
    .await;
    assert_eq!(s, axum::http::StatusCode::OK);
    let actions = body["actions"].as_array().expect("actions array");
    assert_eq!(
        actions.len(),
        2,
        "two envoy actions for a1; the magellan one is filtered out"
    );

    // Find the first action by content and check its tool_calls shape
    let first = actions
        .iter()
        .find(|a| a["reasoning_log"]["data"]["content"] == json!("first thought"))
        .expect("first action");
    assert_eq!(first["tool_calls"].as_array().map(|a| a.len()), Some(1));
    assert_eq!(
        first["tool_calls"][0]["tool_call"]["data"]["tool_name"],
        json!("edit")
    );
}

#[tokio::test]
async fn test_get_actions_unscoped_returns_all_projects() {
    let (app, _td) = setup_test_router();
    req(
        &app,
        axum::http::Method::POST,
        "/atheneum/actions",
        Some(json!({"agent": "shared", "thought": "envoy work", "project_id": "envoy", "tool_calls": []})),
    )
    .await;
    req(
        &app,
        axum::http::Method::POST,
        "/atheneum/actions",
        Some(json!({"agent": "shared", "thought": "magellan work", "project_id": "magellan", "tool_calls": []})),
    )
    .await;

    let (_, body) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/actions?agent=shared",
        None,
    )
    .await;
    let actions = body["actions"].as_array().expect("actions array");
    assert_eq!(
        actions.len(),
        2,
        "no project filter → both actions returned"
    );
}
