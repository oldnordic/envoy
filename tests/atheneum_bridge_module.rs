//! Test module for Envoy-Atheneum Bridge HTTP endpoints
//!
//! This module is only compiled when atheneum is available via dev-dependencies.
//! It provides HTTP interface to atheneum's discovery, handoff, and knowledge APIs.

#![cfg(feature = "atheneum")]

use axum::extract::{Path, Query, State};
use axum::{routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use envoy::engine::Engine;

// Test state with engine and atheneum path
#[derive(Clone)]
pub struct TestState {
    #[allow(dead_code)]
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

pub async fn store_discovery(
    State(state): State<Arc<TestState>>,
    Json(req): Json<StoreDiscoveryRequest>,
) -> Result<(axum::http::StatusCode, Json<StoreDiscoveryResponse>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let agent = req.agent.clone();
    let discovery_type = req.discovery_type.clone();
    let target = req.target.clone();
    let metadata = req.metadata.clone();
    let project_id = req.project_id.clone();

    let discovery_id = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.store_discovery_in_project(
            &agent,
            &discovery_type,
            &target,
            project_id.as_deref(),
            metadata,
        )
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
    let project = query.project.clone();

    let discoveries = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.query_discoveries_in_project(&target, project.as_deref())
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
    let project_id = req.project_id.clone();

    let handoff_id = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.store_handoff_in_project(&from_agent, &to_agent, project_id.as_deref(), manifest)
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
    let project = query.project.clone();

    let handoff = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.get_pending_handoff_in_project(&query.agent, project.as_deref())
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
    let project = query.project.clone();

    let knowledge = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.query_knowledge_in_project(&target, project.as_deref(), None)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;

    // Parse the returned Value into our response structure
    let discovery_count = knowledge["discovery_count"].as_u64().unwrap_or(0) as usize;
    let handoff_count = knowledge["handoff_count"].as_u64().unwrap_or(0) as usize;
    let queried_at = knowledge["queried_at"].as_str().unwrap_or("").to_string();
    let total_entities = knowledge["total_entities"].as_i64().unwrap_or(0);

    // Parse discoveries
    let discoveries: Vec<DiscoveryData> = knowledge["discoveries"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|v| DiscoveryData {
            id: v["id"].as_i64().unwrap_or(0),
            name: v["name"].as_str().unwrap_or("").to_string(),
            data: v.clone(),
        })
        .collect();

    // Parse handoffs
    let handoffs: Vec<HandoffData> = knowledge["handoffs"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|v| {
            let data = v["data"].as_object().cloned().unwrap_or_default();
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

    let savings = &knowledge["token_savings"];
    let token_savings = TokenSavings {
        unique_agents: savings["unique_agents"].as_i64().unwrap_or(0),
        estimated_file_tokens: savings["estimated_file_tokens"].as_i64().unwrap_or(0),
        without_sharing: savings["without_sharing"].as_i64().unwrap_or(0),
        with_sharing: savings["with_sharing"].as_i64().unwrap_or(0),
        saved: savings["saved"].as_i64().unwrap_or(0),
        percentage_reduction: savings["percentage_reduction"].as_f64().unwrap_or(0.0),
    };

    Ok(Json(KnowledgeResponse {
        target: query.target,
        queried_at,
        total_entities,
        discovery_count,
        discoveries,
        handoff_count,
        handoffs,
        token_savings,
    }))
}

// ============================================================================
// Planning + Journal (Stage 8)
// ============================================================================

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

fn parse_status(s: &str) -> Result<atheneum::graph::KanbanStatus, envoy::error::EnvoyError> {
    match s.to_ascii_uppercase().as_str() {
        "TODO" => Ok(atheneum::graph::KanbanStatus::Todo),
        "IN_PROGRESS" | "INPROGRESS" => Ok(atheneum::graph::KanbanStatus::InProgress),
        "DONE" => Ok(atheneum::graph::KanbanStatus::Done),
        "BLOCKED" => Ok(atheneum::graph::KanbanStatus::Blocked),
        other => Err(envoy::error::EnvoyError::Atheneum(anyhow::anyhow!(
            "Unknown KanbanStatus '{}'",
            other
        ))),
    }
}

fn parse_blocker_type(s: &str) -> Result<atheneum::graph::BlockerType, envoy::error::EnvoyError> {
    match s.to_ascii_uppercase().as_str() {
        "DEPENDENCY" => Ok(atheneum::graph::BlockerType::Dependency),
        "BUG" => Ok(atheneum::graph::BlockerType::Bug),
        "INFO_GAP" | "INFOGAP" => Ok(atheneum::graph::BlockerType::InfoGap),
        other => Err(envoy::error::EnvoyError::Atheneum(anyhow::anyhow!(
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

pub async fn create_task(
    State(state): State<Arc<TestState>>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<(axum::http::StatusCode, Json<TaskCreatedResponse>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let task_id = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        g.create_task(
            &req.title,
            req.description.as_deref(),
            req.project_id.as_deref(),
        )
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(TaskCreatedResponse {
            task_id,
            status: "TODO".to_string(),
        }),
    ))
}

pub async fn list_tasks(
    State(state): State<Arc<TestState>>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<ListTasksResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let project = query.project.clone();
    let status_str = query.status.clone();

    let tasks = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<serde_json::Value>> {
        use atheneum::graph::AtheneumGraph;
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        let entities = match status_str {
            Some(s) => {
                let status = match s.to_ascii_uppercase().as_str() {
                    "TODO" => atheneum::graph::KanbanStatus::Todo,
                    "IN_PROGRESS" | "INPROGRESS" => atheneum::graph::KanbanStatus::InProgress,
                    "DONE" => atheneum::graph::KanbanStatus::Done,
                    "BLOCKED" => atheneum::graph::KanbanStatus::Blocked,
                    other => anyhow::bail!("Unknown KanbanStatus '{}'", other),
                };
                g.list_tasks_by_status(status, project.as_deref())?
            }
            None => {
                let all = g.entities_by_kind("Task")?;
                all.into_iter()
                    .filter(|t| match &project {
                        None => true,
                        Some(pid) => t.data.get("project_id").and_then(|v| v.as_str()) == Some(pid),
                    })
                    .collect()
            }
        };
        Ok(entities.into_iter().map(entity_to_json).collect())
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))?
    .map_err(envoy::error::EnvoyError::Atheneum)?;

    Ok(Json(ListTasksResponse { tasks }))
}

pub async fn get_task_details(
    State(state): State<Arc<TestState>>,
    axum::extract::Path(task_id): axum::extract::Path<i64>,
) -> Result<Json<TaskDetailResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let detail = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        g.get_task_with_details(task_id)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;

    Ok(Json(TaskDetailResponse {
        task: entity_to_json(detail.task),
        requirements: detail
            .requirements
            .into_iter()
            .map(entity_to_json)
            .collect(),
        blockers: detail.blockers.into_iter().map(entity_to_json).collect(),
    }))
}

pub async fn patch_task_status(
    State(state): State<Arc<TestState>>,
    axum::extract::Path(task_id): axum::extract::Path<i64>,
    Json(req): Json<UpdateTaskStatusRequest>,
) -> Result<axum::http::StatusCode, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let status = parse_status(&req.status)?;
    tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        g.update_task_status(task_id, status)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;
    Ok(axum::http::StatusCode::OK)
}

pub async fn create_task_requirement(
    State(state): State<Arc<TestState>>,
    axum::extract::Path(task_id): axum::extract::Path<i64>,
    Json(req): Json<CreateRequirementRequest>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let id = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        g.add_requirement(task_id, &req.statement, req.verification_method.as_deref())
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({"requirement_id": id})),
    ))
}

pub async fn create_task_blocker(
    State(state): State<Arc<TestState>>,
    axum::extract::Path(task_id): axum::extract::Path<i64>,
    Json(req): Json<CreateBlockerRequest>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let blocker_type = parse_blocker_type(&req.blocker_type)?;
    let id = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        g.add_blocker(task_id, &req.description, blocker_type)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({"blocker_id": id})),
    ))
}

pub async fn ingest_journal(
    State(state): State<Arc<TestState>>,
    Json(req): Json<IngestJournalRequest>,
) -> Result<(axum::http::StatusCode, Json<IngestJournalResponse>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let (section_ids, applied) = tokio::task::spawn_blocking(
        move || -> anyhow::Result<(Vec<i64>, Vec<serde_json::Value>)> {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
            let ids = g.ingest_journal(&req.path, &req.content, req.project_id.as_deref())?;
            let mut all_applied: Vec<serde_json::Value> = Vec::new();
            for sid in &ids {
                let applied = g.apply_kanban_updates_from_journal(*sid)?;
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
        },
    )
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))?
    .map_err(envoy::error::EnvoyError::Atheneum)?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(IngestJournalResponse {
            section_ids,
            applied_kanban_updates: applied,
        }),
    ))
}

// ============================================================================
// Audit Trail (Stage 9)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ToolCallInput {
    pub tool_name: String,
    pub args: serde_json::Value,
    #[serde(default)]
    pub modified_targets: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateActionRequest {
    pub agent: String,
    pub thought: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallInput>,
}

#[derive(Debug, Serialize)]
pub struct ActionTraceResponse {
    pub agent_id: i64,
    pub reasoning_log_id: i64,
    pub tool_call_ids: Vec<i64>,
    pub modified_edge_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct GetActionsQuery {
    pub agent: String,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GetActionsResponse {
    pub actions: Vec<serde_json::Value>,
}

pub async fn create_action(
    State(state): State<Arc<TestState>>,
    Json(req): Json<CreateActionRequest>,
) -> Result<(axum::http::StatusCode, Json<ActionTraceResponse>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let trace = tokio::task::spawn_blocking(move || {
        use atheneum::graph::{AtheneumGraph, ToolCallRecord};
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        let tool_calls: Vec<ToolCallRecord> = req
            .tool_calls
            .into_iter()
            .map(|tc| ToolCallRecord {
                tool_name: tc.tool_name,
                args: tc.args,
                modified_targets: tc.modified_targets,
            })
            .collect();
        g.record_agent_action(
            &req.agent,
            &req.thought,
            tool_calls,
            req.project_id.as_deref(),
        )
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ActionTraceResponse {
            agent_id: trace.agent_id,
            reasoning_log_id: trace.reasoning_log_id,
            tool_call_ids: trace.tool_call_ids,
            modified_edge_ids: trace.modified_edge_ids,
        }),
    ))
}

pub async fn get_actions(
    State(state): State<Arc<TestState>>,
    Query(query): Query<GetActionsQuery>,
) -> Result<Json<GetActionsResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let agent = query.agent.clone();
    let project = query.project.clone();

    let actions: Vec<serde_json::Value> =
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<serde_json::Value>> {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
            let records = g.get_action_trace(&agent, project.as_deref())?;
            Ok(records
                .into_iter()
                .map(|r| {
                    json!({
                        "reasoning_log": entity_to_json(r.reasoning_log),
                        "tool_calls": r
                            .tool_calls
                            .into_iter()
                            .map(|tc| {
                                json!({
                                    "tool_call": entity_to_json(tc.tool_call),
                                    "modified": tc
                                        .modified
                                        .into_iter()
                                        .map(entity_to_json)
                                        .collect::<Vec<_>>(),
                                })
                            })
                            .collect::<Vec<_>>(),
                    })
                })
                .collect())
        })
        .await
        .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))?
        .map_err(envoy::error::EnvoyError::Atheneum)?;

    Ok(Json(GetActionsResponse { actions }))
}

// ============================================================================
// Ontology (Stage 10)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateClassRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClassCreatedResponse {
    pub class_id: i64,
}

#[derive(Debug, Serialize)]
pub struct ListClassesResponse {
    pub classes: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePropertyRequest {
    pub name: String,
    pub domain_class: String,
    pub range_class: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PropertyCreatedResponse {
    pub property_id: i64,
}

#[derive(Debug, Serialize)]
pub struct ListPropertiesResponse {
    pub properties: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ValidateEdgeQuery {
    pub from: String,
    pub to: String,
    pub edge: String,
}

#[derive(Debug, Serialize)]
pub struct ValidateEdgeResponse {
    pub allowed: bool,
}

#[derive(Debug, Serialize)]
pub struct SeedResponse {
    pub seeded: i64,
}

pub async fn create_class(
    State(state): State<Arc<TestState>>,
    Json(req): Json<CreateClassRequest>,
) -> Result<(axum::http::StatusCode, Json<ClassCreatedResponse>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let id = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        g.define_class(&req.name, req.description.as_deref())
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(ClassCreatedResponse { class_id: id }),
    ))
}

pub async fn list_classes(
    State(state): State<Arc<TestState>>,
) -> Result<Json<ListClassesResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let classes: Vec<serde_json::Value> =
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
            Ok(g.list_classes()?
                .into_iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "name": c.name,
                        "description": c.description,
                    })
                })
                .collect())
        })
        .await
        .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))?
        .map_err(envoy::error::EnvoyError::Atheneum)?;
    Ok(Json(ListClassesResponse { classes }))
}

pub async fn create_property(
    State(state): State<Arc<TestState>>,
    Json(req): Json<CreatePropertyRequest>,
) -> Result<(axum::http::StatusCode, Json<PropertyCreatedResponse>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let id = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        g.define_property(
            &req.name,
            &req.domain_class,
            &req.range_class,
            req.description.as_deref(),
        )
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(PropertyCreatedResponse { property_id: id }),
    ))
}

pub async fn list_properties(
    State(state): State<Arc<TestState>>,
) -> Result<Json<ListPropertiesResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let properties: Vec<serde_json::Value> =
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
            Ok(g.list_properties()?
                .into_iter()
                .map(|p| {
                    json!({
                        "id": p.id,
                        "name": p.name,
                        "domain_class": p.domain_class,
                        "range_class": p.range_class,
                        "description": p.description,
                    })
                })
                .collect())
        })
        .await
        .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))?
        .map_err(envoy::error::EnvoyError::Atheneum)?;
    Ok(Json(ListPropertiesResponse { properties }))
}

pub async fn validate_edge(
    State(state): State<Arc<TestState>>,
    Query(query): Query<ValidateEdgeQuery>,
) -> Result<Json<ValidateEdgeResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let allowed = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        g.validate_edge(&query.from, &query.to, &query.edge)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))??;
    Ok(Json(ValidateEdgeResponse { allowed }))
}

pub async fn seed_ontology(
    State(state): State<Arc<TestState>>,
) -> Result<Json<SeedResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let seeded = tokio::task::spawn_blocking(move || -> anyhow::Result<i64> {
        use atheneum::graph::AtheneumGraph;
        let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        g.seed_standard_ontology()?;
        Ok(g.list_classes()?.len() as i64)
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))?
    .map_err(envoy::error::EnvoyError::Atheneum)?;
    Ok(Json(SeedResponse { seeded }))
}

// ============================================================================
// Code Bridge (Stage 12b)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ImportMagellanSymbolRequest {
    pub magellan_db_path: String,
    pub symbol_name: String,
    pub agent_name: String,
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportMagellanBulkRequest {
    pub magellan_db_path: String,
    pub agent_name: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ImportMagellanSymbolResponse {
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ImportMagellanBulkResponse {
    pub imported_count: i64,
}

pub async fn import_magellan_symbol(
    State(state): State<Arc<TestState>>,
    Json(req): Json<ImportMagellanSymbolRequest>,
) -> Result<(axum::http::StatusCode, Json<ImportMagellanSymbolResponse>), envoy::error::EnvoyError>
{
    let atheneum_path = state.atheneum_path.clone();
    let magellan_path = std::path::PathBuf::from(req.magellan_db_path);
    let symbol_name = req.symbol_name.clone();
    let agent_name = req.agent_name.clone();
    let project_id = req.project_id.clone();

    let result: Option<i64> = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum
            .import_symbol_from_magellan(
                &magellan_path,
                &symbol_name,
                &agent_name,
                project_id.as_deref(),
            )
            .map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))?
    .map_err(envoy::error::EnvoyError::Atheneum)?;

    if let Some(discovery_id) = result {
        Ok((
            axum::http::StatusCode::CREATED,
            Json(ImportMagellanSymbolResponse {
                found: true,
                discovery_id: Some(discovery_id),
            }),
        ))
    } else {
        Ok((
            axum::http::StatusCode::OK,
            Json(ImportMagellanSymbolResponse {
                found: false,
                discovery_id: None,
            }),
        ))
    }
}

pub async fn import_magellan_all(
    State(state): State<Arc<TestState>>,
    Json(req): Json<ImportMagellanBulkRequest>,
) -> Result<(axum::http::StatusCode, Json<ImportMagellanBulkResponse>), envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let magellan_path = std::path::PathBuf::from(req.magellan_db_path);
    let agent_name = req.agent_name.clone();
    let project_id = req.project_id.clone();
    let limit = req.limit;

    let count: usize = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum
            .import_all_symbols_from_magellan(
                &magellan_path,
                &agent_name,
                project_id.as_deref(),
                limit,
            )
            .map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))?
    .map_err(envoy::error::EnvoyError::Atheneum)?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ImportMagellanBulkResponse {
            imported_count: count as i64,
        }),
    ))
}

// ============================================================================
// Router Builder
// ============================================================================

pub async fn get_search(
    State(state): State<Arc<TestState>>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, envoy::error::EnvoyError> {
    let atheneum_path = state.atheneum_path.clone();
    let q = query.q.clone();
    let project = query.project.clone();
    let k = query.k.max(1);

    let results: Vec<SearchResultItem> = tokio::task::spawn_blocking(move || {
        use atheneum::graph::AtheneumGraph;
        let atheneum = AtheneumGraph::open(std::path::Path::new(&atheneum_path))?;
        atheneum.build_search_index()?;
        let hits = atheneum.lexical_search(&q, k, project.as_deref(), None, None)?;
        Ok::<_, anyhow::Error>(
            hits.into_iter()
                .map(|h| SearchResultItem {
                    id: h.id,
                    name: h.name,
                    kind: h.kind,
                    score: h.score,
                    data: h.data,
                })
                .collect(),
        )
    })
    .await
    .map_err(|e| envoy::error::EnvoyError::Atheneum(anyhow::anyhow!("{}", e)))?
    .map_err(envoy::error::EnvoyError::Atheneum)?;

    let count = results.len();
    Ok(Json(SearchResponse {
        query: query.q,
        project: query.project,
        count,
        results,
    }))
}

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
        .route("/atheneum/search", axum::routing::get(get_search))
        .route(
            "/atheneum/tasks",
            axum::routing::post(create_task).get(list_tasks),
        )
        .route("/atheneum/tasks/{id}", axum::routing::get(get_task_details))
        .route(
            "/atheneum/tasks/{id}/status",
            axum::routing::patch(patch_task_status),
        )
        .route(
            "/atheneum/tasks/{id}/requirements",
            axum::routing::post(create_task_requirement),
        )
        .route(
            "/atheneum/tasks/{id}/blockers",
            axum::routing::post(create_task_blocker),
        )
        .route("/atheneum/journals", axum::routing::post(ingest_journal))
        .route(
            "/atheneum/actions",
            axum::routing::post(create_action).get(get_actions),
        )
        .route(
            "/atheneum/ontology/classes",
            axum::routing::post(create_class).get(list_classes),
        )
        .route(
            "/atheneum/ontology/properties",
            axum::routing::post(create_property).get(list_properties),
        )
        .route(
            "/atheneum/ontology/validate",
            axum::routing::get(validate_edge),
        )
        .route(
            "/atheneum/ontology/seed",
            axum::routing::post(seed_ontology),
        )
        .route(
            "/atheneum/import-magellan/symbol",
            axum::routing::post(import_magellan_symbol),
        )
        .route(
            "/atheneum/import-magellan/all",
            axum::routing::post(import_magellan_all),
        )
        .with_state(state)
}
