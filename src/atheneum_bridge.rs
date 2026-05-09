//! Envoy-Atheneum Bridge HTTP endpoints
//!
//! Provides HTTP interface to atheneum's discovery, handoff, and knowledge APIs.

use axum::extract::{Path, Query, State};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::Result;
use crate::http::AppState;

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
    pub queried_at: String,
    pub total_entities: i64,
    pub discovery_count: usize,
    pub discoveries: Vec<DiscoveryData>,
    pub handoff_count: usize,
    pub handoffs: Vec<HandoffData>,
    pub token_savings: TokenSavings,
}

#[derive(Debug, Serialize)]
pub struct TokenSavings {
    pub unique_agents: i64,
    pub estimated_file_tokens: i64,
    pub without_sharing: i64,
    pub with_sharing: i64,
    pub saved: i64,
    pub percentage_reduction: f64,
}

// ============================================================================
// HTTP Handlers
// ============================================================================

/// POST /atheneum/discoveries - Store an agent discovery
pub async fn post_discovery(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StoreDiscoveryRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let agent = req.agent.clone();
    let discovery_type = req.discovery_type.clone();
    let target = req.target.clone();

    let agent2 = agent.clone();
    let discovery_type2 = discovery_type.clone();
    let target2 = target.clone();
    let atheneum_path = state.atheneum_path.clone();

    let discovery_id = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            atheneum
                .store_discovery(&agent2, &discovery_type2, &target2, req.metadata)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(StoreDiscoveryResponse {
            discovery_id,
            agent,
            target,
            discovery_type,
        }),
    ))
}

/// GET /atheneum/discoveries?target=X - Query discoveries by target
pub async fn get_discoveries(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DiscoveriesQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let target = query.target.clone();
    let target2 = target.clone();
    let atheneum_path = state.atheneum_path.clone();

    let discoveries: Vec<DiscoveryData> = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let entities = atheneum
                .query_discoveries(&target2)
                .map_err(crate::error::EnvoyError::from)?;
            Ok(entities
                .into_iter()
                .map(|e| DiscoveryData {
                    id: e.id,
                    name: e.name,
                    data: e.data,
                })
                .collect())
        })
        .await?;

    let discovery_count = discoveries.len();

    Ok(Json(DiscoveriesResponse {
        target,
        discovery_count,
        discoveries,
    }))
}

/// POST /atheneum/handoffs - Store a handoff manifest
pub async fn post_handoff(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StoreHandoffRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let from_agent = req.from_agent.clone();
    let to_agent = req.to_agent.clone();

    let from_agent2 = from_agent.clone();
    let to_agent2 = to_agent.clone();
    let atheneum_path = state.atheneum_path.clone();

    let handoff_id = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            atheneum
                .store_handoff(&from_agent2, &to_agent2, req.manifest)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(StoreHandoffResponse {
            handoff_id,
            from_agent,
            to_agent,
            created_at: chrono::Utc::now().to_rfc3339(),
        }),
    ))
}

/// GET /atheneum/handoffs/pending?agent=X - Get pending handoff for agent
pub async fn get_pending_handoff(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PendingHandoffQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let agent = query.agent.clone();
    let atheneum_path = state.atheneum_path.clone();

    let handoff = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let entity = atheneum
                .get_pending_handoff(&agent)
                .map_err(crate::error::EnvoyError::from)?;
            Ok(entity.map(|e| {
                let data = e.data.as_object().unwrap();
                HandoffData {
                    id: e.id,
                    name: e.name,
                    from_agent: data
                        .get("from_agent")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    to_agent: data
                        .get("to_agent")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    manifest: data.get("manifest").cloned().unwrap_or_default(),
                    created_at: data
                        .get("created_at")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }
            }))
        })
        .await?;

    Ok(Json(PendingHandoffResponse { handoff }))
}

/// POST /atheneum/handoffs/{id}/claim - Mark handoff as claimed
pub async fn claim_handoff(
    State(state): State<Arc<AppState>>,
    Path(handoff_id): Path<i64>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.atheneum_path.clone();

    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            atheneum
                .mark_handoff_claimed(handoff_id)
                .map_err(crate::error::EnvoyError::from)?;
            Ok(())
        })
        .await?;

    Ok(Json(ClaimHandoffResponse {
        claimed: true,
        handoff_id,
    }))
}

/// GET /atheneum/knowledge?target=X - Query aggregated knowledge
pub async fn get_knowledge(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let target = query.target.clone();
    let atheneum_path = state.atheneum_path.clone();

    let knowledge = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let result = atheneum
                .query_knowledge(&target)
                .map_err(crate::error::EnvoyError::from)?;

            // Transform result into response format
            let discoveries: Vec<DiscoveryData> = result["discoveries"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|v| DiscoveryData {
                    id: v["id"].as_i64().unwrap_or(0),
                    name: v["name"].as_str().unwrap_or("").to_string(),
                    data: v.clone(),
                })
                .collect();

            let handoffs: Vec<HandoffData> = result["handoffs"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|v| {
                    let data = v["data"].as_object().unwrap();
                    HandoffData {
                        id: v["id"].as_i64().unwrap_or(0),
                        name: v["name"].as_str().unwrap_or("").to_string(),
                        from_agent: data
                            .get("from_agent")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        to_agent: data
                            .get("to_agent")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        manifest: data.get("manifest").cloned().unwrap_or_default(),
                        created_at: data
                            .get("created_at")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    }
                })
                .collect();

            let savings = &result["token_savings"];
            let token_savings = TokenSavings {
                unique_agents: savings["unique_agents"].as_i64().unwrap_or(0),
                estimated_file_tokens: savings["estimated_file_tokens"].as_i64().unwrap_or(0),
                without_sharing: savings["without_sharing"].as_i64().unwrap_or(0),
                with_sharing: savings["with_sharing"].as_i64().unwrap_or(0),
                saved: savings["saved"].as_i64().unwrap_or(0),
                percentage_reduction: savings["percentage_reduction"].as_f64().unwrap_or(0.0),
            };

            Ok(KnowledgeResponse {
                target,
                queried_at: result["queried_at"].as_str().unwrap_or("").to_string(),
                total_entities: result["total_entities"].as_i64().unwrap_or(0),
                discovery_count: discoveries.len(),
                discoveries,
                handoff_count: handoffs.len(),
                handoffs,
                token_savings,
            })
        })
        .await?;

    Ok(Json(knowledge))
}

// ============================================================================
// Router Builder
// ============================================================================

/// Add atheneum bridge routes to an existing router
pub fn add_atheneum_routes(router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
    router
        .route("/atheneum/discoveries", axum::routing::post(post_discovery))
        .route("/atheneum/discoveries", axum::routing::get(get_discoveries))
        .route("/atheneum/handoffs", axum::routing::post(post_handoff))
        .route(
            "/atheneum/handoffs/pending",
            axum::routing::get(get_pending_handoff),
        )
        .route(
            "/atheneum/handoffs/{id}/claim",
            axum::routing::post(claim_handoff),
        )
        .route("/atheneum/knowledge", axum::routing::get(get_knowledge))
}
