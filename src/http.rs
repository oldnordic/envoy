use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::{routing::get, Json, Router};
use serde::{Deserialize, Serialize};

use crate::agent::AgentRegistry;
use crate::error::{EnvoyError, Result};
use crate::message::{MessageEnvelope, MessageStore, MessageType, Part};

/// Shared application state across all handlers.
pub struct AppState {
    pub agent_registry: AgentRegistry,
    pub message_store: MessageStore,
    pub start_time: chrono::DateTime<chrono::Utc>,
}

impl AppState {
    pub fn new(conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self {
            agent_registry: AgentRegistry::new(),
            message_store: MessageStore::new(conn),
            start_time: chrono::Utc::now(),
        }
    }
}

pub type SharedState = Arc<AppState>;

/// Build the envoy HTTP router.
pub fn build_router(state: SharedState) -> Router {
    Router::new()
        // Agents
        .route("/agents", get(list_agents).post(register_agent))
        .route("/agents/{agent_id}", get(get_agent).delete(disconnect_agent))
        .route("/agents/{agent_id}/messages/pending", get(pending_messages))
        // Messages
        .route("/messages", get(poll_messages).post(send_message))
        .route("/messages/{message_id}", get(get_message))
        // Health
        .route("/health", get(health))
        .route("/stats", get(stats))
        // WebSocket
        .route("/ws/{agent_id}", get(ws_handler))
        .with_state(state)
}

// ── Request/Response types ──

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub kind: String,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub context_id: Option<String>,
    pub parts: Vec<Part>,
}

#[derive(Debug, Deserialize)]
pub struct PollQuery {
    pub to: String,
    #[serde(default)]
    pub since: Option<i64>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, Serialize)]
pub struct PollResponse {
    pub messages: Vec<MessageEnvelope>,
    pub latest_sequence: i64,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub uptime_seconds: i64,
    pub agents_online: usize,
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub messages_total: i64,
    pub agents_registered: usize,
}

// ── Handlers ──

async fn register_agent(
    State(state): State<SharedState>,
    Json(req): Json<RegisterRequest>,
) -> Result<impl IntoResponse> {
    let info = state
        .agent_registry
        .register(&req.name, &req.kind, req.parent_id)?;
    Ok((axum::http::StatusCode::CREATED, Json(info)))
}

async fn disconnect_agent(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let affected = state.agent_registry.disconnect(&agent_id)?;
    Ok(Json(
        serde_json::json!({"disconnected": true, "affected": affected}),
    ))
}

async fn list_agents(
    State(state): State<SharedState>,
) -> Result<impl IntoResponse> {
    let agents = state.agent_registry.list_all();
    Ok(Json(serde_json::json!({"agents": agents})))
}

async fn get_agent(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let info = state.agent_registry.get(&agent_id)?;
    let children = state
        .agent_registry
        .get_children(&agent_id)
        .unwrap_or_default();
    let child_ids: Vec<String> = children.iter().map(|c| c.agent_id.clone()).collect();
    Ok(Json(serde_json::json!({
        "agent_id": info.agent_id,
        "name": info.name,
        "kind": info.kind,
        "online": info.online,
        "parent_id": info.parent_id,
        "children": child_ids,
    })))
}

async fn pending_messages(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    // Tombstone: return undelivered messages for a disconnected agent
    let messages = state.message_store.poll(&agent_id, 0, 100)?;
    Ok(Json(serde_json::json!({
        "messages": messages,
        "count": messages.len()
    })))
}

async fn send_message(
    State(state): State<SharedState>,
    Json(req): Json<SendMessageRequest>,
) -> Result<impl IntoResponse> {
    // Verify sender exists and is online
    let sender = state.agent_registry.get(&req.from)?;
    if !sender.online {
        return Err(EnvoyError::AgentOffline(req.from));
    }

    // Verify recipient exists
    let _recipient = state.agent_registry.get(&req.to)?;

    let envelope = MessageEnvelope {
        message_id: String::new(),
        msg_type: req.msg_type,
        from: req.from,
        to: req.to,
        task_id: req.task_id,
        context_id: req.context_id,
        timestamp: String::new(),
        sequence_id: 0,
        parts: req.parts,
    };

    let stored = state.message_store.store(envelope)?;

    Ok((axum::http::StatusCode::CREATED, Json(stored)))
}

async fn get_message(
    State(state): State<SharedState>,
    Path(message_id): Path<String>,
) -> Result<impl IntoResponse> {
    let msg = state.message_store.get(&message_id)?;
    Ok(Json(msg))
}

async fn poll_messages(
    State(state): State<SharedState>,
    Query(query): Query<PollQuery>,
) -> Result<impl IntoResponse> {
    // Verify recipient exists
    let _ = state.agent_registry.get(&query.to)?;

    let since = query.since.unwrap_or(0);
    let limit = query.limit.min(100);

    let messages = state.message_store.poll(&query.to, since, limit)?;
    let latest_seq = messages
        .last()
        .map(|m| m.sequence_id)
        .unwrap_or(since);

    Ok(Json(PollResponse {
        messages,
        latest_sequence: latest_seq,
    }))
}

async fn health(
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let uptime = (chrono::Utc::now() - state.start_time).num_seconds();
    let online = state.agent_registry.list_online().len();

    Json(HealthResponse {
        status: "ok",
        uptime_seconds: uptime,
        agents_online: online,
    })
}

async fn stats(
    State(state): State<SharedState>,
) -> Result<impl IntoResponse> {
    let total = state.message_store.count_all().unwrap_or(0);
    let registered = state.agent_registry.list_all().len();

    Ok(Json(StatsResponse {
        messages_total: total,
        agents_registered: registered,
    }))
}

// WebSocket — placeholder, implemented in Task 8
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(_state): State<SharedState>,
    Path(_agent_id): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(|_socket| async {
        // Implemented in Task 8
    })
}
