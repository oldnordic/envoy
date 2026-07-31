//! Tests for envoy → atheneum auto-persistence (Phase 3)
//!
//! Tests that handoff messages are automatically stored to atheneum
//! when envoy's atheneum_path is configured.
//!
//! These tests only run when atheneum is available via dev-dependencies.

#![cfg(feature = "atheneum")]

use http_body_util::BodyExt;
use std::sync::Arc;
use tower::util::ServiceExt;

use envoy::engine::Engine;
use envoy::http::AppState;
use envoy::message::{MessageEnvelope, MessageType};
use envoy::status::{AgentState, AgentStatusSnapshot};
use serde_json::json;

/// Helper to create test AppState with temporary databases and atheneum configured
/// Returns (state, temp_dir, atheneum_path)
fn setup_test_state_with_atheneum() -> (Arc<AppState>, tempfile::TempDir, String) {
    let db_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db_path = db_dir.path().join("test.db");
    let atheneum_path = db_dir.path().join("atheneum.db");

    let engine =
        Engine::open(db_path.to_str().expect("Invalid path")).expect("Failed to open engine");

    let state = Arc::new(
        AppState::new(engine)
            .expect("Failed to create AppState")
            .with_atheneum(Some(
                atheneum_path.to_str().expect("Invalid path").to_string(),
            )),
    );

    (
        state,
        db_dir,
        atheneum_path.to_str().expect("Invalid path").to_string(),
    )
}

/// Helper to create test AppState WITHOUT atheneum
/// Returns the TempDir alongside the state: dropping it would delete the
/// database directory out from under the open engine (SQLite WAL journal
/// creation then fails with "disk I/O error").
fn setup_test_state_without_atheneum() -> (Arc<AppState>, tempfile::TempDir) {
    let db_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db_path = db_dir.path().join("test.db");

    let engine =
        Engine::open(db_path.to_str().expect("Invalid path")).expect("Failed to open engine");

    let state = Arc::new(
        AppState::new(engine).expect("Failed to create AppState"), // with_atheneum(None) or just omit the call - defaults to None
    );
    (state, db_dir)
}

/// Helper to register two test agents
/// Returns (agent_id_1, agent_id_2)
async fn register_test_agents(state: Arc<AppState>) -> (String, String) {
    let state_clone = state.clone();
    state
        .with_engine_async(move |engine| {
            let info1 =
                state_clone
                    .agent_registry
                    .register(engine.graph(), "agent-1", "agent", None)?;
            let info2 =
                state_clone
                    .agent_registry
                    .register(engine.graph(), "agent-2", "agent", None)?;

            // Mark agents as online via heartbeat - use the generated agent_id
            let status = AgentStatusSnapshot {
                state: AgentState::Idle,
                working_on: "test".to_string(),
                task_id: None,
                blocked_reason: None,
                waiting_on_agent: None,
                checkpoint: None,
            };
            state_clone.agent_registry.heartbeat(
                engine.graph(),
                &info1.agent_id,
                status.clone(),
            )?;
            state_clone
                .agent_registry
                .heartbeat(engine.graph(), &info2.agent_id, status)?;

            Ok::<(String, String), envoy::EnvoyError>((info1.agent_id, info2.agent_id))
        })
        .await
        .expect("Failed to register agents")
}

/// Helper to build test router with the given state
fn build_test_router(state: Arc<AppState>) -> axum::Router {
    envoy::http::build_router_unlimited(state)
}

// ============================================================================
// Auto-Persistence Tests
// ============================================================================

#[tokio::test]
async fn test_handoff_auto_stored_to_atheneum() {
    let (state, _temp_dir, atheneum_path) = setup_test_state_with_atheneum();
    let app = build_test_router(state.clone());

    // Register test agents and get their IDs
    let (agent1_id, agent2_id) = register_test_agents(state.clone()).await;

    // Send a handoff message via envoy's /messages endpoint
    let handoff_data = json!({
        "completion_status": "DONE",
        "context_remaining_pct": 15,
        "what_was_done": [{
            "scope": "http handler",
            "change": "added rate limiting",
            "verified": true
        }],
        "what_is_stubbed": [],
        "remaining_work": [],
        "verification_state": {
            "tests_passing": 10,
            "tests_failing": 0,
            "quality_gate": {"passed": true, "blocking": 0, "warnings": 0},
            "cargo_check_passed": true
        },
        "magellan_trace": {
            "files_changed": ["src/http.rs"],
            "symbols_added": ["rate_limit_middleware"],
            "symbols_removed": []
        },
        "grounded_queries_used": ["magellan find rate_limit"]
    });

    let request_body = json!({
        "from": agent1_id,
        "to": agent2_id,
        "type": "handoff",
        "task_id": "task-123",
        "parts": [
            {"data": handoff_data}
        ]
    });

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/messages")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&request_body).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Debug: print response if not 201
    let status = response.status();
    if status != axum::http::StatusCode::CREATED {
        let body = BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        eprintln!(
            "Request body: {}",
            serde_json::to_string(&request_body).unwrap()
        );
        eprintln!("Response status: {}", status);
        eprintln!("Response body: {}", String::from_utf8_lossy(&body));
        panic!("Request failed");
    }

    // Message should be stored in envoy
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let envelope: MessageEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(envelope.msg_type, MessageType::Handoff);
    assert_eq!(envelope.from, agent1_id);
    assert_eq!(envelope.to, agent2_id);

    // NOW VERIFY: Handoff should be auto-stored in atheneum
    // This will fail until we implement the auto-persistence feature
    use atheneum::graph::AtheneumGraph;
    let atheneum =
        AtheneumGraph::open(std::path::Path::new(&atheneum_path)).expect("Failed to open atheneum");

    // Query for pending handoffs for agent2
    // Give a small delay to let the background task complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let pending = atheneum
        .get_pending_handoff(&agent2_id)
        .expect("Failed to query pending handoffs");

    assert!(pending.is_some(), "Handoff should be stored in atheneum");
    let handoff = pending.unwrap();
    assert_eq!(handoff.data["from_agent"].as_str().unwrap(), agent1_id);
    assert_eq!(handoff.data["to_agent"].as_str().unwrap(), agent2_id);
}

#[tokio::test]
async fn test_non_handoff_message_not_stored_to_atheneum() {
    let (state, _temp_dir, atheneum_path) = setup_test_state_with_atheneum();
    let app = build_test_router(state.clone());

    let (agent1_id, agent2_id) = register_test_agents(state.clone()).await;

    // Send a Direct (non-handoff) message
    let request_body = json!({
        "from": agent1_id,
        "to": agent2_id,
        "type": "direct",
        "parts": [
            {"text": "Hello, agent-2!"}
        ]
    });

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/messages")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&request_body).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::CREATED);

    // Verify NO handoff was stored in atheneum for direct messages
    use atheneum::graph::AtheneumGraph;
    let atheneum =
        AtheneumGraph::open(std::path::Path::new(&atheneum_path)).expect("Failed to open atheneum");

    let pending = atheneum
        .get_pending_handoff("agent-2")
        .expect("Failed to query pending handoffs");

    assert!(
        pending.is_none(),
        "Direct messages should not create handoffs in atheneum"
    );
}

#[tokio::test]
async fn test_message_delivery_succeeds_when_atheneum_not_configured() {
    let (state, _db_dir) = setup_test_state_without_atheneum();

    let (agent1_id, agent2_id) = register_test_agents(state.clone()).await;

    let app = build_test_router(state);

    // Send a handoff message - should still work even without atheneum
    let request_body = json!({
        "from": agent1_id,
        "to": agent2_id,
        "type": "handoff",
        "parts": [
            {"data": {"completion_status": "DONE", "context_remaining_pct": 50,
                      "what_was_done": [], "what_is_stubbed": [], "remaining_work": [],
                      "verification_state": {"tests_passing": 0, "tests_failing": 0,
                                           "quality_gate": {"passed": true, "blocking": 0, "warnings": 0},
                                           "cargo_check_passed": true},
                      "magellan_trace": {"files_changed": [], "symbols_added": [], "symbols_removed": []},
                      "grounded_queries_used": []}}
        ]
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/messages")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&request_body).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Message should STILL be delivered successfully
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
}
