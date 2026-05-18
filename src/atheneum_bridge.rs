//! Envoy-Atheneum Bridge HTTP endpoints
//!
//! Provides HTTP interface to atheneum's discovery, handoff, and knowledge APIs.

use axum::extract::{Path, Query, State};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
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
    #[serde(default)]
    pub project_id: Option<String>,
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
    #[serde(default)]
    pub project: Option<String>,
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
    #[serde(default)]
    pub project_id: Option<String>,
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
    #[serde(default)]
    pub project: Option<String>,
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
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_search_k")]
    pub k: usize,
    #[serde(default)]
    pub project: Option<String>,
}

fn default_search_k() -> usize {
    5
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub project: Option<String>,
    pub count: usize,
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub score: f32,
    pub data: serde_json::Value,
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
    let project_id = req.project_id.clone();
    let atheneum_path = state.atheneum_path.clone();

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
    let atheneum_path = state.atheneum_path.clone();

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
    let atheneum_path = state.atheneum_path.clone();

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
    let atheneum_path = state.atheneum_path.clone();

    let handoff = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let entity = atheneum
                .get_pending_handoff_in_project(&agent, project.as_deref())
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

/// GET /atheneum/knowledge?target=X[&project=Y] - Query aggregated knowledge
pub async fn get_knowledge(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let target = query.target.clone();
    let project = query.project.clone();
    let atheneum_path = state.atheneum_path.clone();

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

/// GET /atheneum/search?q=<text>[&k=N&project=Y] - Semantic search
pub async fn get_search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let q = query.q.clone();
    let project = query.project.clone();
    let k = query.k.max(1);
    let atheneum_path = state.atheneum_path.clone();

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

// ============================================================================
// Stage 8 — Planning + Journal HTTP
// ============================================================================
//
// Mirrors the test-helper handlers in tests/atheneum_bridge_module.rs.
// See atheneum::graph for the underlying methods.

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TaskCreatedResponse {
    pub task_id: i64,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListTasksResponse {
    pub tasks: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskStatusRequest {
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct TaskDetailResponse {
    pub task: serde_json::Value,
    pub requirements: Vec<serde_json::Value>,
    pub blockers: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequirementRequest {
    pub statement: String,
    #[serde(default)]
    pub verification_method: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBlockerRequest {
    pub description: String,
    pub blocker_type: String,
}

#[derive(Debug, Deserialize)]
pub struct IngestJournalRequest {
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IngestJournalResponse {
    pub section_ids: Vec<i64>,
    pub applied_kanban_updates: Vec<serde_json::Value>,
}

fn parse_status(s: &str) -> Result<atheneum::graph::KanbanStatus> {
    match s.to_ascii_uppercase().as_str() {
        "TODO" => Ok(atheneum::graph::KanbanStatus::Todo),
        "IN_PROGRESS" | "INPROGRESS" => Ok(atheneum::graph::KanbanStatus::InProgress),
        "DONE" => Ok(atheneum::graph::KanbanStatus::Done),
        "BLOCKED" => Ok(atheneum::graph::KanbanStatus::Blocked),
        other => Err(crate::error::EnvoyError::Atheneum(anyhow::anyhow!(
            "Unknown KanbanStatus '{}'",
            other
        ))),
    }
}

fn parse_blocker_type(s: &str) -> Result<atheneum::graph::BlockerType> {
    match s.to_ascii_uppercase().as_str() {
        "DEPENDENCY" => Ok(atheneum::graph::BlockerType::Dependency),
        "BUG" => Ok(atheneum::graph::BlockerType::Bug),
        "INFO_GAP" | "INFOGAP" => Ok(atheneum::graph::BlockerType::InfoGap),
        other => Err(crate::error::EnvoyError::Atheneum(anyhow::anyhow!(
            "Unknown BlockerType '{}'",
            other
        ))),
    }
}

fn entity_to_json(entity: atheneum::GraphEntity) -> serde_json::Value {
    json!({
        "id": entity.id,
        "kind": entity.kind,
        "name": entity.name,
        "file_path": entity.file_path,
        "data": entity.data,
    })
}

pub async fn post_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.atheneum_path.clone();
    let task_id = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.create_task(&req.title, req.description.as_deref(), req.project_id.as_deref())
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(TaskCreatedResponse {
            task_id,
            status: "TODO".to_string(),
        }),
    ))
}

pub async fn get_tasks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListTasksQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.atheneum_path.clone();
    let project = query.project.clone();
    let status_str = query.status.clone();

    let tasks: Vec<serde_json::Value> = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let entities = match status_str {
                Some(s) => {
                    let status = parse_status(&s)?;
                    g.list_tasks_by_status(status, project.as_deref())
                        .map_err(crate::error::EnvoyError::from)?
                }
                None => {
                    let all = g
                        .entities_by_kind("Task")
                        .map_err(crate::error::EnvoyError::from)?;
                    all.into_iter()
                        .filter(|t| match &project {
                            None => true,
                            Some(pid) => {
                                t.data.get("project_id").and_then(|v| v.as_str()) == Some(pid)
                            }
                        })
                        .collect()
                }
            };
            Ok(entities.into_iter().map(entity_to_json).collect())
        })
        .await?;

    Ok(Json(ListTasksResponse { tasks }))
}

pub async fn get_task_details_route(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<i64>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.atheneum_path.clone();
    let detail = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.get_task_with_details(task_id)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(Json(TaskDetailResponse {
        task: entity_to_json(detail.task),
        requirements: detail.requirements.into_iter().map(entity_to_json).collect(),
        blockers: detail.blockers.into_iter().map(entity_to_json).collect(),
    }))
}

pub async fn patch_task_status_route(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<i64>,
    Json(req): Json<UpdateTaskStatusRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.atheneum_path.clone();
    let status = parse_status(&req.status)?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.update_task_status(task_id, status)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::OK)
}

pub async fn post_task_requirement(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<i64>,
    Json(req): Json<CreateRequirementRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.atheneum_path.clone();
    let id = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.add_requirement(task_id, &req.statement, req.verification_method.as_deref())
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({"requirement_id": id})),
    ))
}

pub async fn post_task_blocker(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<i64>,
    Json(req): Json<CreateBlockerRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.atheneum_path.clone();
    let blocker_type = parse_blocker_type(&req.blocker_type)?;
    let id = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.add_blocker(task_id, &req.description, blocker_type)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({"blocker_id": id})),
    ))
}

pub async fn post_journal(
    State(state): State<Arc<AppState>>,
    Json(req): Json<IngestJournalRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.atheneum_path.clone();
    let (section_ids, applied): (Vec<i64>, Vec<serde_json::Value>) = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let ids = g
                .ingest_journal(&req.path, &req.content, req.project_id.as_deref())
                .map_err(crate::error::EnvoyError::from)?;
            let mut all_applied: Vec<serde_json::Value> = Vec::new();
            for sid in &ids {
                let applied = g
                    .apply_kanban_updates_from_journal(*sid)
                    .map_err(crate::error::EnvoyError::from)?;
                for u in applied {
                    all_applied.push(json!({
                        "task_id": u.task_id,
                        "task_title": u.task_title,
                        "previous_status": u.previous_status.as_str(),
                        "new_status": u.new_status.as_str(),
                    }));
                }
            }
            Ok((ids, all_applied))
        })
        .await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(IngestJournalResponse {
            section_ids,
            applied_kanban_updates: applied,
        }),
    ))
}

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
        .route("/atheneum/search", axum::routing::get(get_search))
        .route(
            "/atheneum/tasks",
            axum::routing::post(post_task).get(get_tasks),
        )
        .route(
            "/atheneum/tasks/{id}",
            axum::routing::get(get_task_details_route),
        )
        .route(
            "/atheneum/tasks/{id}/status",
            axum::routing::patch(patch_task_status_route),
        )
        .route(
            "/atheneum/tasks/{id}/requirements",
            axum::routing::post(post_task_requirement),
        )
        .route(
            "/atheneum/tasks/{id}/blockers",
            axum::routing::post(post_task_blocker),
        )
        .route("/atheneum/journals", axum::routing::post(post_journal))
}
