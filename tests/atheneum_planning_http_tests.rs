//! Stage 8: HTTP-level tests for the planning + journal endpoints.
//!
//! Exposes Stage 7's Task lifecycle and Stage 4's journal ingestion over
//! the envoy bridge so agents can drive the workflow remotely.

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
    let router = atheneum_bridge_module::build_test_router(state);
    (router, db_dir)
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
async fn test_create_task_via_http_defaults_to_todo() {
    let (app, _td) = setup_test_router();

    let (status, body) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/tasks",
        Some(json!({
            "title": "Wire HNSW search",
            "description": "Add /atheneum/search",
            "project_id": "envoy"
        })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    assert!(body["task_id"].as_i64().unwrap_or(0) > 0);
    assert_eq!(body["status"], json!("TODO"));
}

#[tokio::test]
async fn test_update_task_status_via_http() {
    let (app, _td) = setup_test_router();
    let (_, created) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/tasks",
        Some(json!({"title": "T", "project_id": "envoy"})),
    )
    .await;
    let task_id = created["task_id"].as_i64().expect("task_id");

    let (status, _) = req(
        &app,
        axum::http::Method::PATCH,
        &format!("/atheneum/tasks/{}/status", task_id),
        Some(json!({"status": "IN_PROGRESS"})),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let (_, detail) = req(
        &app,
        axum::http::Method::GET,
        &format!("/atheneum/tasks/{}", task_id),
        None,
    )
    .await;
    assert_eq!(detail["task"]["data"]["status"], json!("IN_PROGRESS"));
}

#[tokio::test]
async fn test_list_tasks_filtered_by_status_and_project() {
    let (app, _td) = setup_test_router();

    for (title, project) in [("A", "envoy"), ("B", "envoy"), ("C", "magellan")] {
        req(
            &app,
            axum::http::Method::POST,
            "/atheneum/tasks",
            Some(json!({"title": title, "project_id": project})),
        )
        .await;
    }

    let (_, body) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/tasks?project=envoy&status=TODO",
        None,
    )
    .await;
    let tasks = body["tasks"].as_array().expect("tasks array");
    assert_eq!(
        tasks.len(),
        2,
        "envoy + TODO should give 2 tasks (got {})",
        tasks.len()
    );
}

#[tokio::test]
async fn test_task_details_endpoint_includes_requirements_and_blockers() {
    let (app, _td) = setup_test_router();
    let (_, created) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/tasks",
        Some(json!({"title": "Big task", "project_id": "envoy"})),
    )
    .await;
    let task_id = created["task_id"].as_i64().expect("task_id");

    let (s_req, _) = req(
        &app,
        axum::http::Method::POST,
        &format!("/atheneum/tasks/{}/requirements", task_id),
        Some(json!({"statement": "tests pass", "verification_method": "cargo test"})),
    )
    .await;
    assert_eq!(s_req, axum::http::StatusCode::CREATED);

    let (s_blk, _) = req(
        &app,
        axum::http::Method::POST,
        &format!("/atheneum/tasks/{}/blockers", task_id),
        Some(json!({"description": "waiting", "blocker_type": "DEPENDENCY"})),
    )
    .await;
    assert_eq!(s_blk, axum::http::StatusCode::CREATED);

    let (s_det, detail) = req(
        &app,
        axum::http::Method::GET,
        &format!("/atheneum/tasks/{}", task_id),
        None,
    )
    .await;
    assert_eq!(s_det, axum::http::StatusCode::OK);
    assert_eq!(detail["requirements"].as_array().map(|a| a.len()), Some(1));
    assert_eq!(detail["blockers"].as_array().map(|a| a.len()), Some(1));
}

#[tokio::test]
async fn test_post_journal_auto_applies_kanban_updates() {
    let (app, _td) = setup_test_router();

    // First, create the task the journal will reference
    let (_, created) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/tasks",
        Some(json!({"title": "Wire HNSW search", "project_id": "envoy"})),
    )
    .await;
    let task_id = created["task_id"].as_i64().expect("task_id");

    // POST the journal — should ingest sections AND apply the kanban update
    let (s, body) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/journals",
        Some(json!({
            "path": "journals/2026_05_18.md",
            "content": "## 14:30 | progress\n\"Wire HNSW search\" -> DONE ✅\n",
            "project_id": "envoy"
        })),
    )
    .await;
    assert_eq!(s, axum::http::StatusCode::CREATED);
    let applied = body["applied_kanban_updates"]
        .as_array()
        .expect("applied list");
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0]["task_id"], json!(task_id));
    assert_eq!(applied[0]["new_status"], json!("DONE"));

    // Confirm the task entity itself flipped
    let (_, detail) = req(
        &app,
        axum::http::Method::GET,
        &format!("/atheneum/tasks/{}", task_id),
        None,
    )
    .await;
    assert_eq!(detail["task"]["data"]["status"], json!("DONE"));
}
