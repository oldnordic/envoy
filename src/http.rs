use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::{routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::agent::AgentRegistry;
use crate::dependency::DependencyStore;
use crate::engine::Engine;
use crate::error::{EnvoyError, Result};
use crate::message::{MessageEnvelope, MessageStore, MessageType, Part};
use crate::status::NudgeConfig;

/// Registry of active WebSocket senders, keyed by agent_id.
struct WsRegistry {
    senders: Mutex<HashMap<String, broadcast::Sender<String>>>,
}

impl WsRegistry {
    fn new() -> Self {
        Self {
            senders: Mutex::new(HashMap::new()),
        }
    }

    fn register(&self, agent_id: &str) -> broadcast::Receiver<String> {
        let mut senders = self.senders.lock().unwrap();
        if let Some(tx) = senders.get(agent_id) {
            tx.subscribe()
        } else {
            let (tx, rx) = broadcast::channel(256);
            senders.insert(agent_id.to_string(), tx);
            rx
        }
    }

    fn unregister(&self, agent_id: &str) {
        let mut senders = self.senders.lock().unwrap();
        senders.remove(agent_id);
    }

    fn send_json(&self, agent_id: &str, event_type: &str, data: &serde_json::Value) -> bool {
        let event = serde_json::json!({
            "event": event_type,
            "data": data
        });
        let senders = self.senders.lock().unwrap();
        if let Some(tx) = senders.get(agent_id) {
            tx.send(event.to_string()).is_ok()
        } else {
            false
        }
    }
}

/// Shared application state across all handlers.
pub struct AppState {
    pub agent_registry: AgentRegistry,
    pub dependency_store: DependencyStore,
    pub message_store: MessageStore,
    engine: Arc<Mutex<Engine>>,
    ws_registry: WsRegistry,
    pub nudge_config: Mutex<NudgeConfig>,
    pub start_time: chrono::DateTime<chrono::Utc>,
}

impl AppState {
    pub fn new(engine: Engine) -> Result<Self> {
        let agent_registry = AgentRegistry::new(engine.graph())?;
        Ok(Self {
            agent_registry,
            dependency_store: DependencyStore::new(),
            message_store: MessageStore::new(),
            engine: Arc::new(Mutex::new(engine)),
            ws_registry: WsRegistry::new(),
            nudge_config: Mutex::new(NudgeConfig::default()),
            start_time: chrono::Utc::now(),
        })
    }

    /// Lock the engine and run a closure with access to the shared SqliteGraph.
    pub fn with_graph<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&sqlitegraph::SqliteGraph) -> T,
    {
        let engine = self.engine.lock().unwrap();
        f(engine.graph())
    }
}

/// Background task that checks for stale agents and pushes nudge events.
pub async fn run_nudge_loop(state: Arc<AppState>) {
    loop {
        let interval = {
            let cfg = state.nudge_config.lock().unwrap();
            cfg.check_interval_seconds
        };
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

        let threshold = state.nudge_config.lock().unwrap().stale_threshold_minutes;
        let stale = state.agent_registry.get_stale_agents(threshold);

        for agent in &stale {
            let nudge_data = serde_json::json!({
                "reason": format!(
                    "No heartbeat for {} minutes. Current status: {:?}",
                    threshold,
                    agent.status.as_ref().map(|s| s.state.as_str()).unwrap_or("unknown")
                ),
                "severity": "warning",
                "agent_id": agent.agent_id,
                "last_heartbeat": agent.last_heartbeat_at,
            });
            state
                .ws_registry
                .send_json(&agent.agent_id, "nudge", &nudge_data);

            // Also notify blocked dependents
            if let Ok(engine) = state.engine.lock() {
                let deps = state
                    .dependency_store
                    .find_by_blocker(engine.graph(), &agent.agent_id)
                    .unwrap_or_default();
                for dep in &deps {
                    let unblock_msg = serde_json::json!({
                        "blocker_agent": agent.agent_id,
                        "blocker_status": agent.status.as_ref().map(|s| s.state.as_str()).unwrap_or("unknown"),
                        "message": format!(
                            "Your blocker ({}) may be stalled — no heartbeat for {}m",
                            agent.agent_id, threshold
                        ),
                    });
                    state.ws_registry.send_json(
                        &dep.dependent_agent,
                        "blocker_stale",
                        &unblock_msg,
                    );
                }
            }
        }
    }
}

pub type SharedState = Arc<AppState>;

/// Build the envoy HTTP router.
pub fn build_router(state: SharedState) -> Router {
    Router::new()
        // Agents
        .route("/agents", get(list_agents).post(register_agent))
        .route(
            "/agents/{agent_id}",
            get(get_agent).delete(disconnect_agent),
        )
        .route("/agents/{agent_id}/messages/pending", get(pending_messages))
        // Messages
        .route("/messages", get(poll_messages).post(send_message))
        .route("/messages/{message_id}", get(get_message))
        // Health
        .route("/health", get(health))
        .route("/stats", get(stats))
        // Heartbeat + Nudge
        .route("/heartbeat", axum::routing::post(heartbeat))
        .route("/dependencies", axum::routing::post(create_dependency))
        .route("/dependencies/blocker/{agent_id}", get(get_blocker_deps))
        .route(
            "/dependencies/dependent/{agent_id}",
            get(get_dependent_deps),
        )
        .route(
            "/dependencies/{dep_id}/resolve",
            axum::routing::post(resolve_dependency),
        )
        .route(
            "/nudge-config",
            get(get_nudge_config).post(update_nudge_config),
        )
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

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize)]
struct CreateDependencyRequest {
    dependent_agent: String,
    blocker_agent: String,
    reason: String,
}

// ── Handlers ──

async fn register_agent(
    State(state): State<SharedState>,
    Json(req): Json<RegisterRequest>,
) -> Result<impl IntoResponse> {
    let info = state.with_graph(|g| {
        state
            .agent_registry
            .register(g, &req.name, &req.kind, req.parent_id)
    })?;
    Ok((axum::http::StatusCode::CREATED, Json(info)))
}

async fn disconnect_agent(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let affected = state.with_graph(|g| state.agent_registry.disconnect(g, &agent_id))?;
    Ok(Json(
        serde_json::json!({"disconnected": true, "affected": affected}),
    ))
}

async fn list_agents(State(state): State<SharedState>) -> Result<impl IntoResponse> {
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
    let messages = state.with_graph(|g| state.message_store.poll(g, &agent_id, 0, 100))?;
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
    let recipient = req.to.clone();

    let stored = state.with_graph(|g| {
        state.message_store.store(
            g,
            req.msg_type,
            req.from.clone(),
            req.to.clone(),
            req.task_id.clone(),
            req.context_id.clone(),
            req.parts,
        )
    })?;

    // Push to recipient via WebSocket if connected
    let event_data = serde_json::to_value(&stored).unwrap_or_default();
    state
        .ws_registry
        .send_json(&recipient, "message", &event_data);

    Ok((axum::http::StatusCode::CREATED, Json(stored)))
}

async fn get_message(
    State(state): State<SharedState>,
    Path(message_id): Path<String>,
) -> Result<impl IntoResponse> {
    let msg = state.with_graph(|g| state.message_store.get(g, &message_id))?;
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

    let messages = state.with_graph(|g| state.message_store.poll(g, &query.to, since, limit))?;
    let latest_seq = messages.last().map(|m| m.sequence_id).unwrap_or(since);

    Ok(Json(PollResponse {
        messages,
        latest_sequence: latest_seq,
    }))
}

async fn health(State(state): State<SharedState>) -> impl IntoResponse {
    let uptime = (chrono::Utc::now() - state.start_time).num_seconds();
    let online = state.agent_registry.list_online().len();

    Json(HealthResponse {
        status: "ok",
        uptime_seconds: uptime,
        agents_online: online,
    })
}

async fn stats(State(state): State<SharedState>) -> Result<impl IntoResponse> {
    let total = state.with_graph(|g| state.message_store.count_all(g).unwrap_or(0));
    let registered = state.agent_registry.list_all().len();

    Ok(Json(StatsResponse {
        messages_total: total,
        agents_registered: registered,
    }))
}

async fn heartbeat(
    State(state): State<SharedState>,
    Json(req): Json<crate::status::HeartbeatRequest>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    state
        .agent_registry
        .heartbeat(engine.graph(), &req.agent_id, req.status)?;

    let deps = state
        .dependency_store
        .find_by_blocker(engine.graph(), &req.agent_id)?;
    let mut nudges = Vec::new();
    for dep in &deps {
        nudges.push(crate::status::NudgeMessage {
            reason: format!("Dependent {} may now be unblocked", dep.dependent_agent),
            severity: crate::status::NudgeSeverity::Info,
        });
        let notify = serde_json::json!({
            "blocker_agent": req.agent_id,
            "message": "Your blocker just sent a heartbeat — check if you can proceed",
        });
        state
            .ws_registry
            .send_json(&dep.dependent_agent, "blocker_updated", &notify);
    }
    drop(engine);

    Ok(Json(crate::status::HeartbeatResponse {
        accepted: true,
        nudges,
    }))
}

async fn create_dependency(
    State(state): State<SharedState>,
    Json(req): Json<CreateDependencyRequest>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let dep = state.dependency_store.create(
        engine.graph(),
        req.dependent_agent,
        req.blocker_agent,
        req.reason,
    )?;
    Ok((axum::http::StatusCode::CREATED, Json(dep)))
}

async fn get_blocker_deps(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let deps = state
        .dependency_store
        .find_by_blocker(engine.graph(), &agent_id)?;
    Ok(Json(
        serde_json::json!({ "dependencies": deps, "count": deps.len() }),
    ))
}

async fn get_dependent_deps(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let deps = state
        .dependency_store
        .find_by_dependent(engine.graph(), &agent_id)?;
    Ok(Json(
        serde_json::json!({ "dependencies": deps, "count": deps.len() }),
    ))
}

async fn resolve_dependency(
    State(state): State<SharedState>,
    Path(dep_id): Path<String>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let dep = state.dependency_store.resolve(engine.graph(), &dep_id)?;
    drop(engine);

    let notify = serde_json::json!({
        "dependency_id": dep.dependency_id,
        "message": format!("Dependency on {} is resolved", dep.blocker_agent),
    });
    state
        .ws_registry
        .send_json(&dep.dependent_agent, "dependency_resolved", &notify);

    Ok(Json(dep))
}

async fn update_nudge_config(
    State(state): State<SharedState>,
    Json(cfg): Json<crate::status::NudgeConfig>,
) -> Result<impl IntoResponse> {
    let mut current = state.nudge_config.lock().unwrap();
    *current = cfg.clone();
    Ok(Json(cfg))
}

async fn get_nudge_config(State(state): State<SharedState>) -> Result<impl IntoResponse> {
    let cfg = state.nudge_config.lock().unwrap().clone();
    Ok(Json(cfg))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.agent_registry.is_online(&agent_id) {
        return Err(EnvoyError::AgentOffline(agent_id));
    }

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state, agent_id)))
}

async fn handle_ws(mut socket: WebSocket, state: SharedState, agent_id: String) {
    let mut rx = state.ws_registry.register(&agent_id);

    // Send catch-up: undelivered messages for this agent
    {
        let pending = state.with_graph(|g| state.message_store.poll(g, &agent_id, 0, 100));
        if let Ok(pending) = pending {
            for msg in &pending {
                let event = serde_json::json!({
                    "event": "message",
                    "data": msg
                });
                if socket
                    .send(Message::Text(event.to_string().into()))
                    .await
                    .is_err()
                {
                    state.ws_registry.unregister(&agent_id);
                    return;
                }
            }
        }
    }

    // Send connected event
    let connected = serde_json::json!({
        "event": "agent_connected",
        "data": { "agent_id": &agent_id }
    });
    let _ = socket
        .send(Message::Text(connected.to_string().into()))
        .await;

    loop {
        tokio::select! {
            // Incoming events from broadcast channel
            Ok(event_str) = rx.recv() => {
                if socket.send(Message::Text(event_str.into())).await.is_err() {
                    break;
                }
            }
            // Incoming messages from the client (heartbeats, acknowledgements)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(hb) = serde_json::from_str::<serde_json::Value>(&text) {
                            if hb.get("type").and_then(|v| v.as_str()) == Some("heartbeat") {
                                if let Some(data) = hb.get("data") {
                                    if let Ok(status) = serde_json::from_value::<crate::status::AgentStatusSnapshot>(data.clone()) {
                                        if let Ok(engine) = state.engine.lock() {
                                            let _ = state.agent_registry.heartbeat(engine.graph(), &agent_id, status);
                                        }
                                    }
                                }
                                continue;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    state.ws_registry.unregister(&agent_id);
}
