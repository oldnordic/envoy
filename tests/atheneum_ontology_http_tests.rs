//! Stage 10: HTTP tests for the dynamic-ontology endpoints.
//!
//! Wraps atheneum 0.1.x's `define_class`, `define_property`, `list_classes`,
//! `list_properties`, `validate_edge`, and `seed_standard_ontology`.

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
async fn test_post_class_and_list_round_trip() {
    let (app, _td) = setup_test_router();

    let (s, body) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/ontology/classes",
        Some(json!({
            "name": "Hypothesis",
            "description": "A proposed but unverified explanation"
        })),
    )
    .await;
    assert_eq!(s, axum::http::StatusCode::CREATED);
    assert!(body["class_id"].as_i64().unwrap_or(0) > 0);

    let (s2, listed) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/ontology/classes",
        None,
    )
    .await;
    assert_eq!(s2, axum::http::StatusCode::OK);
    let classes = listed["classes"].as_array().expect("classes array");
    assert!(
        classes.iter().any(|c| c["name"] == json!("Hypothesis")),
        "Hypothesis should appear in list"
    );
}

#[tokio::test]
async fn test_post_class_is_idempotent_by_name() {
    let (app, _td) = setup_test_router();

    let (_, body1) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/ontology/classes",
        Some(json!({"name": "Bug", "description": "first"})),
    )
    .await;
    let (_, body2) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/ontology/classes",
        Some(json!({"name": "Bug", "description": "second"})),
    )
    .await;
    assert_eq!(
        body1["class_id"], body2["class_id"],
        "re-posting same name must return same id (updates in place)"
    );

    let (_, listed) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/ontology/classes",
        None,
    )
    .await;
    let bugs: Vec<_> = listed["classes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["name"] == json!("Bug"))
        .collect();
    assert_eq!(bugs.len(), 1, "must not duplicate by name");
    assert_eq!(bugs[0]["description"], json!("second"));
}

#[tokio::test]
async fn test_post_property_and_list_round_trip() {
    let (app, _td) = setup_test_router();

    let (s, body) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/ontology/properties",
        Some(json!({
            "name": "assigned_to",
            "domain_class": "Agent",
            "range_class": "Task",
            "description": "An agent is assigned to a task"
        })),
    )
    .await;
    assert_eq!(s, axum::http::StatusCode::CREATED);
    assert!(body["property_id"].as_i64().unwrap_or(0) > 0);

    let (_, listed) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/ontology/properties",
        None,
    )
    .await;
    let prop = listed["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == json!("assigned_to"))
        .expect("assigned_to in list");
    assert_eq!(prop["domain_class"], json!("Agent"));
    assert_eq!(prop["range_class"], json!("Task"));
}

#[tokio::test]
async fn test_validate_endpoint_open_mode_when_undefined() {
    let (app, _td) = setup_test_router();
    let (s, body) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/ontology/validate?from=Agent&to=Task&edge=spontaneous_edge",
        None,
    )
    .await;
    assert_eq!(s, axum::http::StatusCode::OK);
    assert_eq!(
        body["allowed"],
        json!(true),
        "undefined edges → open mode → allowed"
    );
}

#[tokio::test]
async fn test_validate_endpoint_enforces_domain_range() {
    let (app, _td) = setup_test_router();
    req(
        &app,
        axum::http::Method::POST,
        "/atheneum/ontology/properties",
        Some(json!({
            "name": "modifies",
            "domain_class": "Agent",
            "range_class": "CodeSymbol"
        })),
    )
    .await;

    let (_, ok) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/ontology/validate?from=Agent&to=CodeSymbol&edge=modifies",
        None,
    )
    .await;
    assert_eq!(ok["allowed"], json!(true));

    let (_, wrong_from) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/ontology/validate?from=Task&to=CodeSymbol&edge=modifies",
        None,
    )
    .await;
    assert_eq!(
        wrong_from["allowed"],
        json!(false),
        "wrong domain → rejected"
    );

    let (_, wrong_to) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/ontology/validate?from=Agent&to=Task&edge=modifies",
        None,
    )
    .await;
    assert_eq!(wrong_to["allowed"], json!(false), "wrong range → rejected");
}

#[tokio::test]
async fn test_seed_endpoint_populates_standard_classes() {
    let (app, _td) = setup_test_router();
    let (s, body) = req(
        &app,
        axum::http::Method::POST,
        "/atheneum/ontology/seed",
        Some(json!({})),
    )
    .await;
    assert_eq!(s, axum::http::StatusCode::OK);
    assert!(body["seeded"].as_i64().unwrap_or(0) >= 15);

    let (_, listed) = req(
        &app,
        axum::http::Method::GET,
        "/atheneum/ontology/classes",
        None,
    )
    .await;
    let names: Vec<&str> = listed["classes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    for required in ["Agent", "Task", "Project", "CodeSymbol", "WikiPage"] {
        assert!(
            names.contains(&required),
            "{} must be seeded (got: {:?})",
            required,
            names
        );
    }
}
