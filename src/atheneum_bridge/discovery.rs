use axum::extract::{Path, Query, State};
use axum::Json;
use std::sync::Arc;

use crate::atheneum_bridge::types::*;
use crate::error::Result;
use crate::http::AppState;

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
    let project_id = req.project_id.clone();
    let atheneum_path = state.require_atheneum_path()?;

    let discovery_id = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            atheneum
                .store_discovery_in_project(
                    &agent2,
                    &discovery_type2,
                    &target2,
                    project_id.as_deref(),
                    req.metadata,
                )
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

/// GET /atheneum/discoveries?target=X[&project=Y] - Query discoveries by target
pub async fn get_discoveries(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DiscoveriesQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let target = query.target.clone();
    let target2 = target.clone();
    let project = query.project.clone();
    let atheneum_path = state.require_atheneum_path()?;

    let discoveries: Vec<DiscoveryData> = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let entities = atheneum
                .query_discoveries_in_project(&target2, project.as_deref())
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
    let project_id = req.project_id.clone();
    let atheneum_path = state.require_atheneum_path()?;

    let handoff_id = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            atheneum
                .store_handoff_in_project(
                    &from_agent2,
                    &to_agent2,
                    project_id.as_deref(),
                    req.manifest,
                )
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

/// GET /atheneum/handoffs/pending?agent=X[&project=Y] - Get pending handoff for agent
pub async fn get_pending_handoff(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PendingHandoffQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let agent = query.agent.clone();
    let project = query.project.clone();
    let atheneum_path = state.require_atheneum_path()?;

    let handoff = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let entity = atheneum
                .get_pending_handoff_in_project(&agent, project.as_deref())
                .map_err(crate::error::EnvoyError::from)?;
            Ok(entity.map(|e| {
                let empty = serde_json::Map::new();
                let data = e.data.as_object().unwrap_or(&empty);
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
    let atheneum_path = state.require_atheneum_path()?;

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

/// GET /atheneum/knowledge?target=X[&project=Y] - Query aggregated knowledge
pub async fn get_knowledge(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let target = query.target.clone();
    let project = query.project.clone();
    let atheneum_path = state.require_atheneum_path()?;

    let knowledge = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let result = atheneum
                .query_knowledge_in_project(&target, project.as_deref())
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

            let empty_map = serde_json::Map::new();
            let empty_arr = vec![];
            let handoffs: Vec<HandoffData> = result["handoffs"]
                .as_array()
                .unwrap_or(&empty_arr)
                .iter()
                .map(|v| {
                    let data = v["data"].as_object().unwrap_or(&empty_map);
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

/// GET /atheneum/search?q=<text>[&k=N&project=Y] - Semantic search
pub async fn get_search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let q = query.q.clone();
    let project = query.project.clone();
    let k = query.k.max(1);
    let atheneum_path = state.require_atheneum_path()?;

    let results: Vec<SearchResultItem> = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            // Rebuild the index on each request — small DBs, in-memory HNSW,
            // and this keeps fresh discoveries searchable without a separate
            // "reindex" endpoint. Swap to lazy/periodic if it ever shows up
            // in a profile.
            atheneum
                .build_search_index()
                .map_err(crate::error::EnvoyError::from)?;
            let hits = atheneum
                .semantic_search(&q, k, project.as_deref())
                .map_err(crate::error::EnvoyError::from)?;
            Ok(hits
                .into_iter()
                .map(|h| SearchResultItem {
                    id: h.id,
                    name: h.name,
                    kind: h.kind,
                    score: h.score,
                    data: h.data,
                })
                .collect())
        })
        .await?;

    let count = results.len();
    Ok(Json(SearchResponse {
        query: query.q,
        project: query.project,
        count,
        results,
    }))
}

/// GET /atheneum/context?project=X&limit=N
/// Returns recent discoveries for a project without requiring a target query.
/// Designed for SubagentStart hook: pulls knowledge and prints it into initial context.
pub async fn get_project_context(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ProjectContextQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let project = query.project.clone();
    let limit = query.limit;
    let atheneum_path = state.require_atheneum_path()?;

    let items: Vec<ProjectContextItem> = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let entities = g
                .recent_project_context(&project, limit)
                .map_err(crate::error::EnvoyError::from)?;
            Ok(entities
                .into_iter()
                .map(|e| {
                    let dtype = e
                        .data
                        .get("discovery_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Discovery")
                        .to_string();
                    let target = e
                        .data
                        .get("target")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&e.name)
                        .to_string();
                    let why = e
                        .data
                        .get("why")
                        .or_else(|| e.data.get("metadata").and_then(|m| m.get("why")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let agent = e
                        .data
                        .get("agent")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    ProjectContextItem { discovery_type: dtype, target, why, agent }
                })
                .collect())
        })
        .await?;

    Ok(Json(ProjectContextResponse {
        project: query.project,
        items,
    }))
}
