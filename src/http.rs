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
use crate::event::bus::EventBus;
use crate::event::{self, EventSeverity, EventType};
use crate::message::{MessageEnvelope, MessageStore, MessageType, Part};
use crate::monitor::{ProjectConfig, ProjectConfigStore, SubscriptionStore};
use crate::status::NudgeConfig;
use crate::task::store::TaskStore;
use crate::task::{self, TaskState};

/// Registry of active WebSocket senders, keyed by agent_id.
pub(crate) struct WsRegistry {
    senders: Mutex<HashMap<String, broadcast::Sender<String>>>,
}

impl WsRegistry {
    pub(crate) fn new() -> Self {
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

    pub(crate) fn send_json(
        &self,
        agent_id: &str,
        event_type: &str,
        data: &serde_json::Value,
    ) -> bool {
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
    pub event_bus: EventBus,
    pub task_store: TaskStore,
    pub subscription_store: SubscriptionStore,
    pub project_config_store: ProjectConfigStore,
    pub(crate) engine: Arc<Mutex<Engine>>,
    pub(crate) ws_registry: WsRegistry,
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
            event_bus: EventBus::new(),
            task_store: TaskStore::new(),
            subscription_store: SubscriptionStore::new(),
            project_config_store: ProjectConfigStore::new(),
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

                // Reclaim tasks claimed by stale agents
                if let Ok(reclaimed) = state
                    .task_store
                    .reclaim_stale(engine.graph(), &agent.agent_id)
                {
                    for task_id in &reclaimed {
                        let reclaim_msg = serde_json::json!({
                            "task_id": task_id,
                            "message": format!("Task reclaimed — {} is stale", agent.agent_id),
                        });
                        state.ws_registry.send_json(
                            &agent.agent_id,
                            "task_reclaimed",
                            &reclaim_msg,
                        );
                    }
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
        // Event Bus
        .route("/events/hook", axum::routing::post(ingest_hook_event))
        .route("/events/gate", axum::routing::post(ingest_gate_event))
        .route("/events/ci", axum::routing::post(ingest_ci_event))
        .route("/events/doc", axum::routing::post(ingest_doc_event))
        .route("/events", get(query_events))
        // Task Board
        .route("/tasks/propose", axum::routing::post(propose_task))
        .route("/tasks/claim-next", axum::routing::post(claim_next_task))
        .route("/tasks/{id}/claim", axum::routing::post(claim_task))
        .route("/tasks/{id}/state", axum::routing::post(update_task_state))
        .route("/tasks/{id}", get(get_task))
        .route("/tasks", get(list_tasks))
        // Subscriptions
        .route("/subscriptions", axum::routing::post(subscribe_agent))
        .route(
            "/subscriptions/{agent_id}/{project}",
            axum::routing::delete(unsubscribe_agent),
        )
        .route("/subscriptions/{agent_id}", get(list_subscriptions))
        // Project Config
        .route(
            "/projects/{name}/config",
            get(get_project_config).post(set_project_config),
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

// ── Event handlers ──

async fn ingest_hook_event(
    State(state): State<SharedState>,
    Json(req): Json<event::HookEventRequest>,
) -> Result<impl IntoResponse> {
    let severity = if req.exit_code == 2 {
        EventSeverity::Blocking
    } else if req.exit_code != 0 {
        EventSeverity::Warning
    } else {
        EventSeverity::Info
    };
    let engine = state.engine.lock().unwrap();
    let event = state.event_bus.ingest(
        engine.graph(),
        req.project.clone(),
        EventType::HookResult,
        severity,
        format!("hook:{}", req.hook_name),
        format!("Hook {} exited {}", req.hook_name, req.exit_code),
        serde_json::json!({
            "hook_name": req.hook_name,
            "exit_code": req.exit_code,
            "output_preview": &req.output[..req.output.len().min(200)],
        }),
    )?;
    drop(engine);
    broadcast_to_project(
        &state,
        &req.project,
        "hook_event",
        &serde_json::to_value(&event).unwrap_or_default(),
    );
    Ok((axum::http::StatusCode::CREATED, Json(event)))
}

async fn ingest_gate_event(
    State(state): State<SharedState>,
    Json(req): Json<event::GateEventRequest>,
) -> Result<impl IntoResponse> {
    let severity = if req.gates_passed < req.gates_total {
        EventSeverity::Warning
    } else {
        EventSeverity::Info
    };
    let engine = state.engine.lock().unwrap();
    let event = state.event_bus.ingest(
        engine.graph(),
        req.project.clone(),
        EventType::GateResult,
        severity,
        "gate:quality".into(),
        format!("{}/{} passed", req.gates_passed, req.gates_total),
        serde_json::json!({
            "gates_passed": req.gates_passed,
            "gates_total": req.gates_total,
            "failures": req.failures,
        }),
    )?;
    drop(engine);
    broadcast_to_project(
        &state,
        &req.project,
        "gate_event",
        &serde_json::to_value(&event).unwrap_or_default(),
    );
    Ok((axum::http::StatusCode::CREATED, Json(event)))
}

async fn ingest_ci_event(
    State(state): State<SharedState>,
    Json(req): Json<event::CiEventRequest>,
) -> Result<impl IntoResponse> {
    let severity = match req.conclusion.as_deref() {
        Some("success") => EventSeverity::Info,
        Some("failure") => EventSeverity::Blocking,
        _ => EventSeverity::Info,
    };
    let engine = state.engine.lock().unwrap();
    let event = state.event_bus.ingest(
        engine.graph(),
        req.project.clone(),
        EventType::CiStatus,
        severity,
        "ci:github".into(),
        format!(
            "CI {}: {}",
            req.run_id,
            req.conclusion.as_deref().unwrap_or("in_progress")
        ),
        serde_json::json!({
            "run_id": req.run_id,
            "status": req.status,
            "conclusion": req.conclusion,
            "head_branch": req.head_branch,
            "display_title": req.display_title,
        }),
    )?;
    drop(engine);
    broadcast_to_project(
        &state,
        &req.project,
        "ci_event",
        &serde_json::to_value(&event).unwrap_or_default(),
    );
    Ok((axum::http::StatusCode::CREATED, Json(event)))
}

async fn ingest_doc_event(
    State(state): State<SharedState>,
    Json(req): Json<event::DocEventRequest>,
) -> Result<impl IntoResponse> {
    let severity = if req.last_updated_seconds > 86400 {
        EventSeverity::Warning
    } else {
        EventSeverity::Info
    };
    let engine = state.engine.lock().unwrap();
    let event = state.event_bus.ingest(
        engine.graph(),
        req.project.clone(),
        EventType::DocSync,
        severity,
        "doc:wiki".into(),
        format!("Docs last updated {}s ago", req.last_updated_seconds),
        serde_json::json!({
            "doc_files": req.doc_files,
            "last_updated_seconds": req.last_updated_seconds,
        }),
    )?;
    drop(engine);
    broadcast_to_project(
        &state,
        &req.project,
        "doc_event",
        &serde_json::to_value(&event).unwrap_or_default(),
    );
    Ok((axum::http::StatusCode::CREATED, Json(event)))
}

#[derive(Debug, Deserialize)]
struct EventQueryParams {
    project: String,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn query_events(
    State(state): State<SharedState>,
    Query(params): Query<EventQueryParams>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let events = state.event_bus.query(
        engine.graph(),
        &params.project,
        params.since.as_deref(),
        Some(params.limit.unwrap_or(50).min(100)),
    )?;
    drop(engine);
    Ok(Json(serde_json::json!({
        "events": events,
        "count": events.len(),
    })))
}

// ── Task handlers ──

async fn propose_task(
    State(state): State<SharedState>,
    Json(req): Json<task::ProposeTaskRequest>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let task = state.task_store.propose(
        engine.graph(),
        req.project.clone(),
        req.description,
        req.blocked_by,
    )?;
    drop(engine);
    broadcast_to_project(
        &state,
        &req.project,
        "task_proposed",
        &serde_json::to_value(&task).unwrap_or_default(),
    );
    Ok((axum::http::StatusCode::CREATED, Json(task)))
}

async fn claim_task(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
    Json(req): Json<task::ClaimTaskRequest>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let task = state
        .task_store
        .claim(engine.graph(), &task_id, req.agent_id)?;
    drop(engine);
    broadcast_to_project(
        &state,
        &task.project,
        "task_claimed",
        &serde_json::to_value(&task).unwrap_or_default(),
    );
    Ok(Json(task))
}

async fn claim_next_task(
    State(state): State<SharedState>,
    Json(req): Json<task::ClaimNextRequest>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let task = state
        .task_store
        .claim_next(engine.graph(), &req.project, req.agent_id)?;
    drop(engine);
    broadcast_to_project(
        &state,
        &task.project,
        "task_claimed",
        &serde_json::to_value(&task).unwrap_or_default(),
    );
    Ok((axum::http::StatusCode::CREATED, Json(task)))
}

async fn update_task_state(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
    Json(req): Json<task::UpdateTaskStateRequest>,
) -> Result<impl IntoResponse> {
    let new_state = TaskState::from_str(&req.state)
        .ok_or_else(|| EnvoyError::InvalidMessage(format!("unknown state: {}", req.state)))?;
    let is_done = new_state == TaskState::Done;
    let engine = state.engine.lock().unwrap();
    let task =
        state
            .task_store
            .update_state(engine.graph(), &task_id, new_state, req.checkpoint, None)?;
    if is_done {
        let blocked = state.task_store.find_blocked_by(engine.graph(), &task_id)?;
        for bt in &blocked {
            let notify = serde_json::json!({
                "resolved_dependency": task_id,
                "task_id": bt.id,
                "message": format!("Dependency {} resolved — can proceed", task_id),
            });
            if let Some(ref claimant) = bt.claimed_by {
                state
                    .ws_registry
                    .send_json(claimant, "dependency_resolved", &notify);
            }
        }
    }
    drop(engine);
    broadcast_to_project(
        &state,
        &task.project,
        "task_state_changed",
        &serde_json::to_value(&task).unwrap_or_default(),
    );
    Ok(Json(task))
}

#[derive(Debug, Deserialize)]
struct ListTasksQuery {
    project: String,
    #[serde(default)]
    state: Option<String>,
}

async fn list_tasks(
    State(state): State<SharedState>,
    Query(params): Query<ListTasksQuery>,
) -> Result<impl IntoResponse> {
    let filter = params.state.as_deref().and_then(TaskState::from_str);
    let engine = state.engine.lock().unwrap();
    let tasks = state
        .task_store
        .list(engine.graph(), &params.project, filter.as_ref())?;
    drop(engine);
    Ok(Json(serde_json::json!({
        "tasks": tasks,
        "count": tasks.len(),
    })))
}

async fn get_task(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let task = state.task_store.get(engine.graph(), &task_id)?;
    drop(engine);
    Ok(Json(task))
}

// ── Subscription handlers ──

async fn subscribe_agent(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse> {
    let agent_id = body["agent_id"].as_str().unwrap_or("");
    let project = body["project"].as_str().unwrap_or("");
    if agent_id.is_empty() || project.is_empty() {
        return Err(EnvoyError::InvalidMessage(
            "agent_id and project required".into(),
        ));
    }
    let engine = state.engine.lock().unwrap();
    state
        .subscription_store
        .subscribe(engine.graph(), agent_id, project)?;
    drop(engine);
    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({"subscribed": true, "agent_id": agent_id, "project": project})),
    ))
}

async fn unsubscribe_agent(
    State(state): State<SharedState>,
    Path((agent_id, project)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    state
        .subscription_store
        .unsubscribe(engine.graph(), &agent_id, &project)?;
    drop(engine);
    Ok(Json(serde_json::json!({"unsubscribed": true})))
}

async fn list_subscriptions(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let subs = state.subscription_store.list(engine.graph(), &agent_id)?;
    drop(engine);
    Ok(Json(
        serde_json::json!({"agent_id": agent_id, "subscriptions": subs}),
    ))
}

// ── Project config handlers ──

async fn get_project_config(
    State(state): State<SharedState>,
    Path(project): Path<String>,
) -> Result<impl IntoResponse> {
    let engine = state.engine.lock().unwrap();
    let cfg = state.project_config_store.get(engine.graph(), &project)?;
    drop(engine);
    Ok(Json(cfg))
}

async fn set_project_config(
    State(state): State<SharedState>,
    Path(project): Path<String>,
    Json(cfg): Json<ProjectConfig>,
) -> Result<impl IntoResponse> {
    let mut cfg = cfg;
    cfg.project = project.clone();
    let engine = state.engine.lock().unwrap();
    state.project_config_store.set(engine.graph(), &cfg)?;
    drop(engine);
    Ok(Json(
        serde_json::json!({"configured": true, "project": project}),
    ))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    // WS connection is proof of being online. Mark connecting agent as active.
    // Auto-registers if this is a first-time agent.
    if !state.agent_registry.is_online(&agent_id) {
        let engine = state.engine.lock().unwrap();
        drop(engine);
    }
    {
        let engine = state.engine.lock().unwrap();
        let _ = state.agent_registry.heartbeat(
            engine.graph(),
            &agent_id,
            crate::status::AgentStatusSnapshot {
                state: crate::status::AgentState::Working,
                task_id: None,
                blocked_reason: None,
                waiting_on_agent: None,
                checkpoint: Some("ws_connected".into()),
                working_on: "connected via WS".into(),
            },
        );
    }

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state, agent_id)))
}

async fn handle_ws(mut socket: WebSocket, state: SharedState, agent_id: String) {
    let mut rx = state.ws_registry.register(&agent_id);

    // Catch-up: undelivered messages for this agent
    {
        let pending = state.with_graph(|g| state.message_store.poll(g, &agent_id, 0, 100));
        if let Ok(pending) = pending {
            for msg in &pending {
                let event = serde_json::json!({"event": "message", "data": msg});
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

    // Catch-up: recent events for subscribed projects (reconnect replay)
    {
        let since = (chrono::Utc::now() - chrono::Duration::minutes(15)).to_rfc3339();
        // Collect all catch-up payloads first (no await across MutexGuard)
        let catchup_events: Vec<serde_json::Value> = {
            let engine = state.engine.lock().unwrap();
            let projects = state
                .subscription_store
                .list(engine.graph(), &agent_id)
                .unwrap_or_default();
            let mut payloads = Vec::new();
            for project in &projects {
                if let Ok(events) =
                    state
                        .event_bus
                        .query(engine.graph(), project, Some(&since), Some(50))
                {
                    for evt in &events {
                        if let Ok(payload) = serde_json::to_value(evt) {
                            payloads.push(
                                serde_json::json!({"event": "event_catchup", "data": payload}),
                            );
                        }
                    }
                }
            }
            payloads
        };
        for msg in &catchup_events {
            if socket
                .send(Message::Text(msg.to_string().into()))
                .await
                .is_err()
            {
                state.ws_registry.unregister(&agent_id);
                return;
            }
        }
    }

    // Connected event
    let connected = serde_json::json!({
        "event": "agent_connected",
        "data": { "agent_id": &agent_id }
    });
    let _ = socket
        .send(Message::Text(connected.to_string().into()))
        .await;

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event_str) => {
                        if socket.send(Message::Text(event_str.into())).await.is_err() {
                            break;
                        }
                    }
                    // Channel overflowed — re-subscribe at current position
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        let _ = socket.send(Message::Text(
                            serde_json::json!({
                                "event": "channel_lagged",
                                "data": { "skipped": n }
                            }).to_string().into()
                        )).await;
                        rx = state.ws_registry.register(&agent_id);
                    }
                    Err(_) => break, // channel closed
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(hb) = serde_json::from_str::<serde_json::Value>(&text) {
                            match hb.get("type").and_then(|v| v.as_str()) {
                                Some("heartbeat") => {
                                    if let Some(data) = hb.get("data") {
                                        if let Ok(status) = serde_json::from_value::<crate::status::AgentStatusSnapshot>(data.clone()) {
                                            let engine = state.engine.lock().unwrap();
                                            let _ = state.agent_registry.heartbeat(engine.graph(), &agent_id, status);
                                            drop(engine);
                                        }
                                    }
                                    continue;
                                }
                                Some("ping") => {
                                    let _ = socket.send(Message::Text(
                                        serde_json::json!({"type": "pong"}).to_string().into()
                                    )).await;
                                    continue;
                                }
                                _ => {}
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

/// Broadcast an event to all agents subscribed to a project.
fn broadcast_to_project(
    state: &SharedState,
    project: &str,
    event_type: &str,
    data: &serde_json::Value,
) {
    let subs = {
        let engine = state.engine.lock().unwrap();
        state
            .subscription_store
            .subscribers(engine.graph(), project)
            .unwrap_or_default()
    };
    for agent_id in subs {
        state.ws_registry.send_json(&agent_id, event_type, data);
    }
}
