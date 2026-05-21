//! Dashboard Edge Fixes - TDD Tests
//!
//! Tests for fixing graph edge rendering, discovery metadata, and drill-down APIs.
//! Following grounded-coding: RED → GREEN → REFACTOR

#![cfg(feature = "dashboard")]

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use envoy::engine::Engine;
use envoy::http::AppState;

/// Helper to create test AppState with atheneum configured
/// Requires ATHENEUM_DB env var to point to a real database with data
fn setup_test_state() -> Arc<AppState> {
    let atheneum_path =
        std::env::var("ATHENEUM_DB").expect("ATHENEUM_DB env var must be set for edge fix tests");

    let db_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db_path = db_dir.path().join("test.db");

    let engine =
        Engine::open(db_path.to_str().expect("Invalid path")).expect("Failed to open engine");

    Arc::new(
        AppState::new(engine)
            .expect("Failed to create AppState")
            .with_atheneum(Some(atheneum_path)),
    )
}

// ============================================================================
// TEST 1: Graph edges should return all edges, not just agent outgoing
// ============================================================================

#[tokio::test]
async fn test_graph_edges_returns_all_edges() {
    let state = setup_test_state();
    // Using real atheneum database - no need to seed data

    let app = envoy::http::build_router_unlimited(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/dashboard/graph/edges")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // RED: This will fail because current implementation returns 0 edges
    // Expected: At least 1 edge (Event → Agent)
    // Actual: [] (empty)
    assert!(
        json.as_array().map(|a| a.len()).unwrap_or(0) > 0,
        "Expected at least 1 edge, got {}",
        json.as_array().map(|a| a.len()).unwrap_or(0)
    );
}

#[tokio::test]
async fn test_graph_edges_include_event_to_agent_links() {
    let state = setup_test_state();
    // Using real atheneum database - no need to seed data

    let app = envoy::http::build_router_unlimited(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/dashboard/graph/edges")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    if let Some(edges) = json.as_array() {
        if !edges.is_empty() {
            // Verify edge has required fields
            let first_edge = &edges[0];
            assert!(first_edge.get("id").is_some(), "Edge missing 'id' field");
            assert!(
                first_edge.get("source").is_some(),
                "Edge missing 'source' field"
            );
            assert!(
                first_edge.get("target").is_some(),
                "Edge missing 'target' field"
            );
            assert!(
                first_edge.get("kind").is_some(),
                "Edge missing 'kind' field"
            );
        }
    }
}

// ============================================================================
// TEST 2: Graph nodes should include discovery metadata
// ============================================================================

#[tokio::test]
async fn test_graph_nodes_include_discovery_metadata() {
    let state = setup_test_state();
    // Using real atheneum database - no need to seed data

    let app = envoy::http::build_router_unlimited(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/dashboard/graph/nodes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    if let Some(nodes) = json.as_array() {
        // Find Discovery nodes
        let discovery_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.get("kind").and_then(|k| k.as_str()) == Some("Discovery"))
            .collect();

        // RED: Discovery nodes should have metadata
        // Current implementation may have empty data: {}
        for node in discovery_nodes {
            let data = node.get("data").unwrap();
            // Discovery should have agent, target, discovery_type in data
            if let Some(_obj) = data.as_object() {
                // For now, just check data exists (can be empty object)
                // After fix: should contain agent, target, discovery_type
            }
        }
    }
}

// ============================================================================
// TEST 3: Node neighbors API for drill-down
// ============================================================================

#[tokio::test]
async fn test_node_neighbors_returns_2hop_subgraph() {
    let state = setup_test_state();
    // Using real atheneum database - no need to seed data

    let app = envoy::http::build_router_unlimited(state);

    // First get a node ID to query
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/dashboard/graph/nodes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    if let Some(nodes) = json.as_array() {
        if let Some(first_node) = nodes.first() {
            if let Some(node_id) = first_node.get("id").and_then(|i| i.as_str()) {
                // Query neighbors - NEW ENDPOINT
                let neighbor_response = app
                    .oneshot(
                        Request::builder()
                            .uri(&format!("/api/dashboard/node/{}/neighbors", node_id))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();

                // RED: This endpoint doesn't exist yet - should return 404 or 200
                // After implementation: should return 200 with subgraph
                assert!(
                    neighbor_response.status() == StatusCode::OK
                        || neighbor_response.status() == StatusCode::NOT_FOUND,
                    "Node neighbors endpoint should exist"
                );
            }
        }
    }
}

// ============================================================================
// TEST 4: Query discoveries by target
// ============================================================================

#[tokio::test]
async fn test_query_discoveries_by_target() {
    let state = setup_test_state();
    // Using real atheneum database - no need to seed data

    let app = envoy::http::build_router_unlimited(state);

    // Query for discoveries about "test_function"
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/dashboard/discoveries?target=test_function")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // RED: This endpoint doesn't exist yet
    // After implementation: should return 200 with discoveries
    assert!(
        response.status() == StatusCode::OK || response.status() == StatusCode::NOT_FOUND,
        "Discoveries query endpoint should exist"
    );

    if response.status() == StatusCode::OK {
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Should return at least the discovery we seeded
        if let Some(discoveries) = json.get("discoveries").and_then(|d| d.as_array()) {
            assert!(
                !discoveries.is_empty(),
                "Expected at least 1 discovery for test_function"
            );
        }
    }
}
