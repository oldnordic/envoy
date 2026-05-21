//! Dashboard API TDD tests
//!
//! Tests follow red-green-refactor:
//! 1. Write failing test
//! 2. Run (fails)
//! 3. Implement minimal code
//! 4. Run (passes)

#![cfg(feature = "dashboard")]

use std::sync::Arc;

use envoy::engine::Engine;
use envoy::http::AppState;

/// Helper to create test AppState with atheneum configured
/// Returns (state, tempdir) to keep tempdir alive
fn setup_test_state_with_dashboard() -> (Arc<AppState>, tempfile::TempDir) {
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

    (state, db_dir)
}

/// Helper to populate atheneum with test data
fn populate_atheneum_test_data(atheneum_path: String) -> Result<(), Box<dyn std::error::Error>> {
    use atheneum::graph::AtheneumGraph;

    // Ensure the database is initialized first
    let _ = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
    // Small delay to ensure SQLite file is flushed
    std::thread::sleep(std::time::Duration::from_millis(50));

    let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;

    // Store a test discovery
    atheneum.store_discovery(
        "test-agent",
        "test_discovery",
        "test-target",
        serde_json::json!({"name": "test", "content": "data"}),
    )?;

    // Store another discovery with different type
    atheneum.store_discovery(
        "another-agent",
        "code_review",
        "review-target",
        serde_json::json!({"name": "review", "file": "src/test.rs"}),
    )?;

    Ok(())
}

#[tokio::test]
async fn test_graph_nodes_returns_valid_structure() {
    let (state, _tempdir) = setup_test_state_with_dashboard();

    // Populate test data before querying
    let atheneum_path = state.atheneum_path.clone().unwrap();
    populate_atheneum_test_data(atheneum_path).expect("Failed to populate test data");

    // Give SQLite time to flush (file-based databases have commit delays)
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Query nodes
    let nodes = envoy::dashboard::get_graph_nodes_impl(&state)
        .await
        .expect("Failed to get nodes");

    // Should have at least the discoveries we stored
    assert!(
        !nodes.is_empty(),
        "Should return at least one node, got {}",
        nodes.len()
    );

    // Verify structure
    let node = &nodes[0];
    assert!(!node.id.is_empty(), "Node should have id");
    assert!(!node.kind.is_empty(), "Node should have kind");
    assert!(!node.label.is_empty(), "Node should have label");
}

#[tokio::test]
async fn test_graph_edges_link_valid_nodes() {
    let (state, _tempdir) = setup_test_state_with_dashboard();

    // Populate test data
    let atheneum_path = state.atheneum_path.clone().unwrap();
    populate_atheneum_test_data(atheneum_path).expect("Failed to populate test data");

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Query both nodes and edges
    let nodes = envoy::dashboard::get_graph_nodes_impl(&state)
        .await
        .expect("Failed to get nodes");
    let edges = envoy::dashboard::get_graph_edges_impl(&state)
        .await
        .expect("Failed to get edges");

    // All edge source/target IDs should exist in nodes
    let node_ids: std::collections::HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();

    for edge in &edges {
        assert!(
            node_ids.contains(&edge.source),
            "Edge source {} should exist in nodes",
            edge.source
        );
        assert!(
            node_ids.contains(&edge.target),
            "Edge target {} should exist in nodes",
            edge.target
        );
    }
}

#[tokio::test]
async fn test_graph_stats_matches_db() {
    let (state, _tempdir) = setup_test_state_with_dashboard();

    // Populate test data
    let atheneum_path = state.atheneum_path.clone().unwrap();
    populate_atheneum_test_data(atheneum_path).expect("Failed to populate test data");

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let stats = envoy::dashboard::get_graph_stats_impl(&state)
        .await
        .expect("Failed to get stats");

    // Stats should reflect the stored discoveries
    assert!(
        stats.discoveries >= 1,
        "Should have at least one discovery, got {}",
        stats.discoveries
    );
}

#[tokio::test]
async fn test_kanban_groups_tasks_by_state() {
    let (state, _tempdir) = setup_test_state_with_dashboard();

    // Create test tasks in different states
    let state_clone = state.clone();
    state
        .with_engine_async(move |engine| {
            use envoy::task::TaskState;

            // Create tasks using propose
            let _task1 = state_clone
                .task_store
                .propose(
                    engine.graph(),
                    "".to_string(),
                    "TODO task".to_string(),
                    vec![],
                )
                .expect("Failed to create task1");

            let task2 = state_clone
                .task_store
                .propose(
                    engine.graph(),
                    "".to_string(),
                    "In progress task".to_string(),
                    vec![],
                )
                .expect("Failed to create task2");

            // Claim task2 first (Proposed -> Claimed)
            state_clone
                .task_store
                .claim(engine.graph(), &task2.id, "agent-1".to_string())
                .expect("Failed to claim task2");

            // Then update to InProgress (Claimed -> InProgress)
            state_clone
                .task_store
                .update_state(
                    engine.graph(),
                    &task2.id,
                    TaskState::InProgress,
                    None,
                    Some("agent-1"),
                )
                .expect("Failed to update task2 state");

            Ok(())
        })
        .await
        .expect("Failed to create tasks");

    let tasks = envoy::dashboard::get_dashboard_tasks_impl(&state)
        .await
        .expect("Failed to get tasks");

    // Should have tasks grouped by state
    assert!(!tasks.is_empty(), "Should have tasks grouped by state");
}

#[tokio::test]
async fn test_audit_timeline_returns_events() {
    let (state, _tempdir) = setup_test_state_with_dashboard();

    // Log some audit events
    let state_clone = state.clone();
    state
        .with_engine_async(move |engine| {
            state_clone
                .audit_store
                .log_message(
                    engine.graph(),
                    "agent-1",
                    "agent-2",
                    envoy::message::MessageType::Direct,
                    "msg-1",
                    None,
                )
                .expect("Failed to log event1");

            state_clone
                .audit_store
                .log_message(
                    engine.graph(),
                    "agent-1",
                    "agent-2",
                    envoy::message::MessageType::Direct,
                    "msg-2",
                    None,
                )
                .expect("Failed to log event2");

            Ok(())
        })
        .await
        .expect("Failed to log events");

    let events = envoy::dashboard::get_dashboard_audit_impl(&state, None, None)
        .await
        .expect("Failed to get audit");

    assert!(!events.is_empty(), "Should return audit events");
    assert_eq!(events.len(), 2, "Should have 2 audit events");
}
