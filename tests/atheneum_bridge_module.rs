//! Test module for Envoy-Atheneum Bridge HTTP endpoints
//!
//! This module is only compiled when atheneum is available via dev-dependencies.
//! It provides HTTP interface to atheneum's discovery, handoff, and knowledge APIs.

#![cfg(feature = "atheneum")]

use axum::extract::{Path, Query, State};
use axum::{routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use envoy::engine::Engine;

// Test state with engine and atheneum path
#[derive(Clone)]
pub struct TestState {
    pub engine: Arc<std::sync::Mutex<Engine>>,
    pub atheneum_path: String,
}

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct StoreDiscoveryRequest {
    pub agent: String,
    pub discovery_type: String,
    pub target: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct StoreDiscoveryResponse {
    pub discovery_id: i64,
    pub agent: String,
    pub target: String,
    pub discovery_type: String,
}

#[derive(Debug, Deserialize)]
pub struct DiscoveriesQuery {
    pub target: String,
}

#[derive(Debug, Serialize)]
pub struct DiscoveriesResponse {
    pub target: String,
    pub discovery_count: usize,
    pub discoveries: Vec<DiscoveryData>,
}

#[derive(Debug, Serialize)]
pub struct DiscoveryData {
    pub id: i64,
    pub name: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct StoreHandoffRequest {
    pub from_agent: String,
    pub to_agent: String,
    pub manifest: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct StoreHandoffResponse {
    pub handoff_id: i64,
    pub from_agent: String,
    pub to_agent: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct PendingHandoffQuery {
    pub agent: String,
}

#[derive(Debug, Serialize)]
pub struct PendingHandoffResponse {
    pub handoff: Option<HandoffData>,
}

#[derive(Debug, Serialize)]
pub struct HandoffData {
    pub id: i64,
    pub name: String,
    pub from_agent: String,
    pub to_agent: String,
    pub manifest: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ClaimHandoffResponse {
    pub claimed: bool,
    pub handoff_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct KnowledgeQuery {
    pub target: String,
}

#[derive(Debug, Serialize)]
pub struct KnowledgeResponse {
    pub target: String,
    pub discovery_count: usize,
    pub token_savings: TokenSavings,
}

#[derive(Debug, Serialize)]
pub struct TokenSavings {
    pub total: usize,
    pub by_type: std::collections::HashMap<String, usize>,
}

// ============================================================================
// HTTP Handlers
// ============================================================================

pub async fn store_discovery(
    State(state): State<Arc<TestState>>,
    Json(req): Json<StoreDiscoveryRequest>,
) -> Result<(axum::http::StatusCode, Json<StoreDiscoveryResponse>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let agent = req.agent.clone();
    let discovery_type = req.discovery_type.clone();
    let target = req.target.clone();
    let metadata = req.metadata.clone();

    let discovery_id = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.store_discovery(&agent, &discovery_type, &target, metadata)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(StoreDiscoveryResponse {
            discovery_id,
            agent: req.agent,
            target: req.target,
            discovery_type: req.discovery_type,
        }),
    ))
}

pub async fn get_discoveries(
    State(state): State<Arc<TestState>>,
    Query(query): Query<DiscoveriesQuery>,
) -> Result<Json<DiscoveriesResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let target = query.target.clone();

    let discoveries = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.query_discoveries(&target)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;

    let discovery_count = discoveries.len();
    let discoveries: Vec<DiscoveryData> = discoveries
        .into_iter()
        .map(|d| DiscoveryData {
            id: d.id,
            name: d.name,
            data: d.data,
        })
        .collect();

    Ok(Json(DiscoveriesResponse {
        target: query.target,
        discovery_count,
        discoveries,
    }))
}

pub async fn store_handoff(
    State(state): State<Arc<TestState>>,
    Json(req): Json<StoreHandoffRequest>,
) -> Result<(axum::http::StatusCode, Json<StoreHandoffResponse>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let from_agent = req.from_agent.clone();
    let to_agent = req.to_agent.clone();
    let manifest = req.manifest.clone();

    let handoff_id = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.store_handoff(&from_agent, &to_agent, manifest)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(StoreHandoffResponse {
            handoff_id,
            from_agent: req.from_agent,
            to_agent: req.to_agent,
            created_at: chrono::Utc::now().to_rfc3339(),
        }),
    ))
}

pub async fn get_pending_handoff(
    State(state): State<Arc<TestState>>,
    Query(query): Query<PendingHandoffQuery>,
) -> Result<Json<PendingHandoffResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();

    let handoff = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.get_pending_handoff(&query.agent)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;

    let handoff = handoff.map(|h| {
        let from_agent = h.data["from_agent"].as_str().unwrap_or("").to_string();
        let to_agent = h.data["to_agent"].as_str().unwrap_or("").to_string();
        let manifest = h
            .data
            .get("manifest")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        HandoffData {
            id: h.id,
            name: h.name,
            from_agent,
            to_agent,
            manifest,
            created_at: chrono::Utc::now().to_rfc3339(), // Fallback since entity doesn't have created_at
        }
    });

    Ok(Json(PendingHandoffResponse { handoff }))
}

pub async fn claim_handoff(
    State(state): State<Arc<TestState>>,
    Path(handoff_id): Path<i64>,
) -> Result<Json<ClaimHandoffResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();

    let claimed = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.mark_handoff_claimed(handoff_id)?;
        Result::<bool, anyhow::Error>::Ok(true)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;

    Ok(Json(ClaimHandoffResponse {
        claimed,
        handoff_id,
    }))
}

pub async fn get_knowledge(
    State(state): State<Arc<TestState>>,
    Query(query): Query<KnowledgeQuery>,
) -> Result<Json<KnowledgeResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let target = query.target.clone();

    let knowledge = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.query_knowledge(&target)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;

    // Parse the returned Value into our response structure
    let discovery_count = knowledge["discovery_count"].as_u64().unwrap_or(0) as usize;
    let total = knowledge["token_savings"]["total"].as_u64().unwrap_or(0) as usize;
    let mut by_type = std::collections::HashMap::new();
    if let Some(obj) = knowledge["token_savings"]["by_type"].as_object() {
        for (k, v) in obj {
            if let Some(count) = v.as_u64() {
                by_type.insert(k.clone(), count as usize);
            }
        }
    }

    Ok(Json(KnowledgeResponse {
        target: query.target,
        discovery_count,
        token_savings: TokenSavings { total, by_type },
    }))
}

// ============================================================================
// Router Builder
// ============================================================================

pub fn build_test_router(state: Arc<TestState>) -> Router {
    Router::new()
        .route(
            "/atheneum/discoveries",
            post(store_discovery).get(get_discoveries),
        )
        .route("/atheneum/handoffs", post(store_handoff))
        .route(
            "/atheneum/handoffs/pending",
            axum::routing::get(get_pending_handoff),
        )
        .route(
            "/atheneum/handoffs/{id}/claim",
            axum::routing::post(claim_handoff),
        )
        .route("/atheneum/knowledge", axum::routing::get(get_knowledge))
        .with_state(state)
}
