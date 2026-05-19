//! Stage 12b: HTTP tests for the magellan code-bridge endpoints.

#![cfg(feature = "atheneum")]

mod atheneum_bridge_module;

use http_body_util::BodyExt;
use rusqlite::{params, Connection};
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

/// Build a tiny magellan-shaped sqlitegraph DB so we can drive the
/// bridge endpoints without needing a real magellan installation.
fn build_fake_magellan_db(symbols: &[(&str, &str, i64)]) -> std::path::PathBuf {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("magellan.db");
    let conn = Connection::open(&path).expect("open");
    conn.execute_batch(
        "CREATE TABLE graph_entities (
             id INTEGER PRIMARY KEY,
             kind TEXT NOT NULL,
             name TEXT NOT NULL,
             file_path TEXT,
             data TEXT
         );",
    )
    .expect("schema");
    for (name, file, line) in symbols {
        conn.execute(
            "INSERT INTO graph_entities (kind, name, file_path, data) VALUES ('Symbol', ?1, ?2, ?3)",
            params![
                name,
                file,
                serde_json::to_string(&json!({
                    "fqn": format!("crate::{}", name),
                    "name": name,
                    "kind": "Function",
                    "start_line": line,
                    "end_line": line + 5
                }))
                .unwrap()
            ],
        )
        .expect("insert");
    }
    // Leak the tempdir so the file survives for the duration of the test.
    // Tests don't run long enough for this to matter, and the OS will
    // reap /tmp eventually.
    std::mem::forget(dir);
    path
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
async fn test_import_magellan_symbol_via_http() {
    let (app, _td) = setup_test_router();
    let magellan_path = build_fake_magellan_db(&[
        ("build_router", "src/http.rs", 555),
        ("parse_yaml", "src/graph/mod.rs", 1523),
    ]);

    let (status, body) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/import-magellan/symbol",
        Some(json!({
            "magellan_db_path": magellan_path.to_str().unwrap(),
            "symbol_name": "build_router",
            "agent_name": "claude1",
            "project_id": "envoy"
        })),
    )
    .await;

    assert_eq!(status, axum::http::StatusCode::CREATED);
    assert_eq!(body["found"], json!(true));
    assert!(body["discovery_id"].as_i64().unwrap_or(0) > 0);
}

#[tokio::test]
async fn test_import_magellan_symbol_not_found_returns_found_false() {
    let (app, _td) = setup_test_router();
    let magellan_path = build_fake_magellan_db(&[("only_one", "src/lib.rs", 1)]);

    let (status, body) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/import-magellan/symbol",
        Some(json!({
            "magellan_db_path": magellan_path.to_str().unwrap(),
            "symbol_name": "doesnt_exist",
            "agent_name": "claude1"
        })),
    )
    .await;

    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["found"], json!(false));
    assert!(body["discovery_id"].is_null());
}

#[tokio::test]
async fn test_import_magellan_bulk_via_http() {
    let (app, _td) = setup_test_router();
    let magellan_path = build_fake_magellan_db(&[
        ("a", "x.rs", 1),
        ("b", "x.rs", 10),
        ("c", "y.rs", 1),
        ("d", "y.rs", 20),
    ]);

    let (status, body) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/import-magellan/all",
        Some(json!({
            "magellan_db_path": magellan_path.to_str().unwrap(),
            "agent_name": "claude1",
            "project_id": "envoy"
        })),
    )
    .await;

    assert_eq!(status, axum::http::StatusCode::CREATED);
    assert_eq!(body["imported_count"], json!(4));
}

#[tokio::test]
async fn test_import_magellan_bulk_respects_limit() {
    let (app, _td) = setup_test_router();
    let magellan_path =
        build_fake_magellan_db(&[("a", "x.rs", 1), ("b", "x.rs", 10), ("c", "y.rs", 1)]);

    let (_, body) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/import-magellan/all",
        Some(json!({
            "magellan_db_path": magellan_path.to_str().unwrap(),
            "agent_name": "claude1",
            "limit": 2
        })),
    )
    .await;
    assert_eq!(body["imported_count"], json!(2));
}
