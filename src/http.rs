use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use axum::{routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::agent::AgentRegistry;
use crate::atheneum_bridge;
use crate::circuit;
use crate::dependency::DependencyStore;
use crate::engine::Engine;
use crate::error::{EnvoyError, Result};
use crate::event::bus::{DeliveryTracker, EventBus};
use crate::event::{self, EventSeverity, EventType};
use crate::message::{MessageEnvelope, MessageStore, MessageType, Part};
use crate::monitor::{ProjectConfig, ProjectConfigStore, SubscriptionStore};
use crate::rate_limit::{HybridRateLimiter, RateLimitConfig};
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
    pub audit_store: crate::audit::AuditStore,
    pub dependency_store: DependencyStore,
    pub message_store: MessageStore,
    pub event_bus: EventBus,
    pub delivery_tracker: DeliveryTracker,
    pub task_store: TaskStore,
    pub subscription_store: SubscriptionStore,
    pub project_config_store: ProjectConfigStore,
    pub circuit_breaker: circuit::CircuitBreaker,
    pub(crate) engine: Arc<Mutex<Engine>>,
    pub(crate) ws_registry: WsRegistry,
    pub rate_limiter: HybridRateLimiter,
    pub nudge_config: Mutex<NudgeConfig>,
    pub start_time: chrono::DateTime<chrono::Utc>,
    /// Path to the atheneum database for agent knowledge sharing
    pub atheneum_path: String,
}

impl AppState {
    pub fn new(engine: Engine) -> Result<Self> {
        Self::with_atheneum_path(engine, ".magellan/atheneum.db")
    }

    pub fn with_atheneum_path(engine: Engine, atheneum_path: &str) -> Result<Self> {
        let agent_registry = AgentRegistry::new(engine.graph())?;
        let rate_limiter = HybridRateLimiter::new(
            engine.graph(),
            RateLimitConfig::default(),
            1000, // L1 capacity
        )?;
        Ok(Self {
            agent_registry,
            audit_store: crate::audit::AuditStore::new(),
            dependency_store: DependencyStore::new(),
            message_store: MessageStore::new(),
            event_bus: EventBus::new(),
            delivery_tracker: DeliveryTracker::new(),
            task_store: TaskStore::new(),
            subscription_store: SubscriptionStore::new(),
            project_config_store: ProjectConfigStore::new(),
            circuit_breaker: circuit::CircuitBreaker::with_defaults(),
            engine: Arc::new(Mutex::new(engine)),
            ws_registry: WsRegistry::new(),
            rate_limiter,
            nudge_config: Mutex::new(NudgeConfig::default()),
            start_time: chrono::Utc::now(),
            atheneum_path: atheneum_path.to_string(),
        })
    }

    /// Async version of with_graph — offloads DB work to the blocking thread pool.
    /// Use this from all async handlers to avoid blocking tokio worker threads.
    pub async fn with_graph_async<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&sqlitegraph::SqliteGraph) -> T + Send + 'static,
        T: Send + 'static,
    {
        let engine = self.engine.clone();
        let result = tokio::task::spawn_blocking(move || {
            let engine = engine.lock().unwrap();
            f(engine.graph())
        })
        .await
        .map_err(|_| EnvoyError::InvalidEntity("blocking task panicked".into()))?;
        Ok(result)
    }

    /// Async version that provides the full Engine (not just graph).
    pub async fn with_engine_async<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Engine) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || {
            let engine = engine.lock().unwrap();
            f(&engine)
        })
        .await
        .map_err(|_| EnvoyError::InvalidEntity("blocking task panicked".into()))?
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

            // Fetch blocked dependents + reclaim stale tasks via blocking pool
            let state_fb = state.clone();
            let agent_id_fb = agent.agent_id.clone();
            let (deps, reclaimed) = tokio::task::spawn_blocking(move || {
                let engine = state_fb.engine.lock().unwrap();
                let deps = state_fb
                    .dependency_store
                    .find_by_blocker(engine.graph(), &agent_id_fb)
                    .unwrap_or_default();
                let reclaimed = state_fb
                    .task_store
                    .reclaim_stale(engine.graph(), &agent_id_fb)
                    .unwrap_or_default();
                (deps, reclaimed)
            })
            .await
            .unwrap_or((Vec::new(), Vec::new()));

            // WS sends are in-memory
            for dep in &deps {
                let unblock_msg = serde_json::json!({
                    "blocker_agent": agent.agent_id,
                    "blocker_status": agent.status.as_ref().map(|s| s.state.as_str()).unwrap_or("unknown"),
                    "message": format!(
                        "Your blocker ({}) may be stalled — no heartbeat for {}m",
                        agent.agent_id, threshold
                    ),
                });
                state
                    .ws_registry
                    .send_json(&dep.dependent_agent, "blocker_stale", &unblock_msg);
            }
            for task_id in &reclaimed {
                let reclaim_msg = serde_json::json!({
                    "task_id": task_id,
                    "message": format!("Task reclaimed — {} is stale", agent.agent_id),
                });
                state
                    .ws_registry
                    .send_json(&agent.agent_id, "task_reclaimed", &reclaim_msg);
            }
        }
    }
}

pub type SharedState = Arc<AppState>;

/// Rate limiting middleware using HybridRateLimiter.
///
/// Extracts agent ID from X-Agent-Id header (if present), otherwise uses
/// a fallback identifier. Checks rate limits and returns 429 if exceeded.
pub async fn rate_limit_middleware(
    State(state): State<SharedState>,
    request: Request,
    next: Next,
) -> Response {
    // Extract agent ID from header or use fallback
    let agent_id = request
        .headers()
        .get("x-agent-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous");

    // Check rate limit (with graph reference)
    let decision = {
        let engine = state.engine.lock().unwrap();
        state
            .rate_limiter
            .check_rate_limit(engine.graph(), agent_id)
    };

    if !decision.allowed {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }

    next.run(request).await
}

/// Build the base envoy HTTP routes (without state).
fn build_base_routes() -> Router<SharedState> {
    Router::new()
        .route("/agents", get(list_agents).post(register_agent))
        .route(
            "/agents/{agent_id}",
            get(get_agent).delete(disconnect_agent),
        )
        .route("/agents/{agent_id}/messages/pending", get(pending_messages))
        .route("/messages", get(poll_messages).post(send_message))
        .route("/messages/{message_id}", get(get_message))
        .route(
            "/messages/{message_id}/ack",
            axum::routing::post(ack_message),
        )
        .route("/agents/{agent_id}/circuit", get(get_circuit))
        .route(
            "/agents/{agent_id}/circuit/failure",
            axum::routing::post(record_circuit_failure),
        )
        .route("/health", get(health))
        .route("/stats", get(stats))
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
        .route("/events/hook", axum::routing::post(ingest_hook_event))
        .route("/events/gate", axum::routing::post(ingest_gate_event))
        .route("/events/ci", axum::routing::post(ingest_ci_event))
        .route("/events/doc", axum::routing::post(ingest_doc_event))
        .route("/events/verify", axum::routing::post(ingest_verify_event))
        .route("/events", get(query_events))
        .route("/audit", get(query_audit))
        .route("/tasks/propose", axum::routing::post(propose_task))
        .route("/tasks/claim-next", axum::routing::post(claim_next_task))
        .route("/tasks/{id}/claim", axum::routing::post(claim_task))
        .route("/tasks/{id}/state", axum::routing::post(update_task_state))
        .route("/tasks/{id}/audit", get(query_task_audit))
        .route("/tasks/{id}", get(get_task))
        .route("/tasks", get(list_tasks))
        .route("/subscriptions", axum::routing::post(subscribe_agent))
        .route(
            "/subscriptions/{agent_id}/{project}",
            axum::routing::delete(unsubscribe_agent),
        )
        .route("/subscriptions/{agent_id}", get(list_subscriptions))
        .route(
            "/projects/{name}/config",
            get(get_project_config).post(set_project_config),
        )
        .route("/ws/{agent_id}", get(ws_handler))
}

/// Build the envoy HTTP router with rate limiting.
pub fn build_router(state: SharedState) -> Router {
    build_base_routes()
        .with_state(state.clone())
        .layer(axum::extract::DefaultBodyLimit::max(1_048_576))
        .layer(axum::middleware::from_fn_with_state(
            state,
            rate_limit_middleware,
        ))
}

/// Build the envoy HTTP router with atheneum routes (uses rate limiting).
pub fn build_router_with_atheneum(state: SharedState) -> Router {
    atheneum_bridge::add_atheneum_routes(build_base_routes())
        .with_state(state.clone())
        .layer(axum::extract::DefaultBodyLimit::max(1_048_576))
        .layer(axum::middleware::from_fn_with_state(
            state,
            rate_limit_middleware,
        ))
}

/// Build the router without rate limiting (for tests).
pub fn build_router_unlimited(state: SharedState) -> Router {
    build_base_routes().with_state(state)
}

/// Build the router with atheneum routes without rate limiting (for tests).
pub fn build_router_unlimited_with_atheneum(state: SharedState) -> Router {
    atheneum_bridge::add_atheneum_routes(build_base_routes()).with_state(state)
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
    #[serde(default)]
    pub include: Option<String>,
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
    let state_fb = state.clone();
    let info = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        let info = state_fb.agent_registry.register(
            engine.graph(),
            &req.name,
            &req.kind,
            req.parent_id,
        )?;
        let _ = state_fb.audit_store.log_agent_registered(
            engine.graph(),
            &info.agent_id,
            &info.name,
            &info.kind,
        );
        Ok::<_, crate::error::EnvoyError>(info)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok((axum::http::StatusCode::CREATED, Json(info)))
}

async fn disconnect_agent(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let aid = agent_id.clone();
    let affected = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        let affected = state_fb.agent_registry.disconnect(engine.graph(), &aid)?;
        let _ = state_fb
            .audit_store
            .log_agent_disconnected(engine.graph(), &aid);
        Ok::<_, crate::error::EnvoyError>(affected)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    state.circuit_breaker.remove(&agent_id);
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
    let state_fb = state.clone();
    let messages = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .message_store
            .poll(engine.graph(), &agent_id, 0, 100, true)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(serde_json::json!({
        "messages": messages,
        "count": messages.len()
    })))
}

async fn send_message(
    State(state): State<SharedState>,
    Json(req): Json<SendMessageRequest>,
) -> Result<impl IntoResponse> {
    // Verify sender exists and is online (in-memory)
    let sender = state.agent_registry.get(&req.from)?;
    if !sender.online {
        return Err(EnvoyError::AgentOffline(req.from));
    }

    // Reject self-messaging
    if req.from == req.to {
        return Err(EnvoyError::InvalidMessage(
            "cannot send message to self".into(),
        ));
    }

    // Verify recipient exists (in-memory)
    let _recipient = state.agent_registry.get(&req.to)?;
    let recipient = req.to.clone();

    let state_fb = state.clone();
    let stored = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        let stored = state_fb.message_store.store(
            engine.graph(),
            req.msg_type.clone(),
            req.from.clone(),
            req.to.clone(),
            req.task_id.clone(),
            req.context_id.clone(),
            req.parts,
        )?;
        let _ = state_fb.audit_store.log_message(
            engine.graph(),
            &stored.from,
            &stored.to,
            stored.msg_type.clone(),
            &stored.message_id,
            None,
        );
        Ok::<_, crate::error::EnvoyError>(stored)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;

    // Push to recipient via WebSocket if connected (in-memory)
    let event_data = serde_json::to_value(&stored).unwrap_or_default();
    match state.circuit_breaker.check(&recipient) {
        circuit::CanDeliver::Yes | circuit::CanDeliver::Probe => {
            let delivered = state
                .ws_registry
                .send_json(&recipient, "message", &event_data);
            if delivered {
                state.circuit_breaker.record_success(&recipient);
                let state_fb = state.clone();
                let recipient_fb = recipient.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let engine = state_fb.engine.lock().unwrap();
                    state_fb
                        .audit_store
                        .log_circuit_closed(engine.graph(), &recipient_fb)
                })
                .await;
            } else {
                state.circuit_breaker.record_failure(&recipient);
                let status = state.circuit_breaker.get_state(&recipient);
                if status.state == "open" {
                    let state_fb = state.clone();
                    let recipient_fb = recipient.clone();
                    let failures = status.failures;
                    let _ = tokio::task::spawn_blocking(move || {
                        let engine = state_fb.engine.lock().unwrap();
                        state_fb.audit_store.log_circuit_opened(
                            engine.graph(),
                            &recipient_fb,
                            failures,
                        )
                    })
                    .await;
                }
            }
        }
        circuit::CanDeliver::No => {
            // Circuit is open — message stored but not pushed. Agent will catch up on reconnect.
        }
    }

    Ok((axum::http::StatusCode::CREATED, Json(stored)))
}

async fn get_message(
    State(state): State<SharedState>,
    Path(message_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let msg = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb.message_store.get(engine.graph(), &message_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(msg))
}

async fn poll_messages(
    State(state): State<SharedState>,
    Query(query): Query<PollQuery>,
) -> Result<impl IntoResponse> {
    // Verify recipient exists (in-memory)
    let _ = state.agent_registry.get(&query.to)?;

    let since = query.since.unwrap_or(0);
    let limit = query.limit.clamp(1, 100);
    let include_acked = query.include.as_deref() == Some("acked");

    let state_fb = state.clone();
    let to = query.to.clone();
    let messages = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .message_store
            .poll(engine.graph(), &to, since, limit, include_acked)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    let latest_seq = messages.last().map(|m| m.sequence_id).unwrap_or(since);

    Ok(Json(PollResponse {
        messages,
        latest_sequence: latest_seq,
    }))
}

#[derive(Debug, Deserialize)]
pub struct AckRequest {
    pub agent_id: String,
}

async fn ack_message(
    State(state): State<SharedState>,
    Path(message_id): Path<String>,
    Json(req): Json<AckRequest>,
) -> Result<impl IntoResponse> {
    // Verify agent exists (in-memory)
    let _ = state.agent_registry.get(&req.agent_id)?;

    let state_fb = state.clone();
    let mid = message_id.clone();
    let acked = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .message_store
            .ack(engine.graph(), &mid, &req.agent_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;

    Ok(Json(serde_json::json!({
        "message_id": message_id,
        "acked_by": acked,
    })))
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
    let state_fb = state.clone();
    let total = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .message_store
            .count_all(engine.graph())
            .unwrap_or(0)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))?;
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
    let agent_id = req.agent_id.clone();
    let status = req.status.clone();

    // Offload DB work to blocking pool — clone Arc<AppState> for the closure
    let state_for_blocking = state.clone();
    let deps = tokio::task::spawn_blocking(move || {
        let engine = state_for_blocking.engine.lock().unwrap();
        state_for_blocking
            .agent_registry
            .heartbeat(engine.graph(), &agent_id, status)?;
        state_for_blocking
            .dependency_store
            .find_by_blocker(engine.graph(), &agent_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("heartbeat blocking task panicked".into()))??;

    // Circuit breaker and WS sends are in-memory — no blocking
    state.circuit_breaker.reset(&req.agent_id);

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

    Ok(Json(crate::status::HeartbeatResponse {
        accepted: true,
        nudges,
    }))
}

async fn get_circuit(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let _ = state.agent_registry.get(&agent_id)?;
    let status = state.circuit_breaker.get_state(&agent_id);
    Ok(Json(serde_json::json!({
        "agent_id": status.agent_id,
        "state": status.state,
        "failure_count": status.failures,
        "opened_at": status.opened_at,
    })))
}

async fn record_circuit_failure(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let _ = state.agent_registry.get(&agent_id)?;
    state.circuit_breaker.record_failure(&agent_id);
    let status = state.circuit_breaker.get_state(&agent_id);
    Ok(Json(serde_json::json!({
        "agent_id": status.agent_id,
        "state": status.state,
        "failure_count": status.failures,
    })))
}

async fn create_dependency(
    State(state): State<SharedState>,
    Json(req): Json<CreateDependencyRequest>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let dep = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb.dependency_store.create(
            engine.graph(),
            req.dependent_agent,
            req.blocker_agent,
            req.reason,
        )
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok((axum::http::StatusCode::CREATED, Json(dep)))
}

async fn get_blocker_deps(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let deps = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .dependency_store
            .find_by_blocker(engine.graph(), &agent_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(
        serde_json::json!({ "dependencies": deps, "count": deps.len() }),
    ))
}

async fn get_dependent_deps(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let deps = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .dependency_store
            .find_by_dependent(engine.graph(), &agent_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(
        serde_json::json!({ "dependencies": deps, "count": deps.len() }),
    ))
}

async fn resolve_dependency(
    State(state): State<SharedState>,
    Path(dep_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let dep = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb.dependency_store.resolve(engine.graph(), &dep_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;

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
    let state_fb = state.clone();
    let event = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        let event = state_fb.event_bus.ingest(
            engine.graph(),
            req.project.clone(),
            EventType::HookResult,
            severity,
            format!("hook:{}", req.hook_name),
            format!("Hook {} exited {}", req.hook_name, req.exit_code),
            serde_json::json!({
                "hook_name": req.hook_name,
                "exit_code": req.exit_code,
                "output_preview": req.output.chars().take(200).collect::<String>(),
            }),
        )?;
        let _ = state_fb.audit_store.log_event_ingested(
            engine.graph(),
            &req.project,
            &format!("hook:{}", req.hook_name),
            EventType::HookResult,
        );
        Ok::<_, crate::error::EnvoyError>(event)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    broadcast_to_project(
        &state,
        &event.project,
        "hook_event",
        &serde_json::to_value(&event).unwrap_or_default(),
    )
    .await;
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
    let state_fb = state.clone();
    let event = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        let event = state_fb.event_bus.ingest(
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
        let _ = state_fb.audit_store.log_event_ingested(
            engine.graph(),
            &req.project,
            "gate:quality",
            EventType::GateResult,
        );
        Ok::<_, crate::error::EnvoyError>(event)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    broadcast_to_project(
        &state,
        &event.project,
        "gate_event",
        &serde_json::to_value(&event).unwrap_or_default(),
    )
    .await;
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
    let state_fb = state.clone();
    let event = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        let event = state_fb.event_bus.ingest(
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
        let _ = state_fb.audit_store.log_event_ingested(
            engine.graph(),
            &req.project,
            "ci:github",
            EventType::CiStatus,
        );
        Ok::<_, crate::error::EnvoyError>(event)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    broadcast_to_project(
        &state,
        &event.project,
        "ci_event",
        &serde_json::to_value(&event).unwrap_or_default(),
    )
    .await;
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
    let state_fb = state.clone();
    let event = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        let event = state_fb.event_bus.ingest(
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
        let _ = state_fb.audit_store.log_event_ingested(
            engine.graph(),
            &req.project,
            "doc:wiki",
            EventType::DocSync,
        );
        Ok::<_, crate::error::EnvoyError>(event)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    broadcast_to_project(
        &state,
        &event.project,
        "doc_event",
        &serde_json::to_value(&event).unwrap_or_default(),
    )
    .await;
    Ok((axum::http::StatusCode::CREATED, Json(event)))
}

async fn ingest_verify_event(
    State(state): State<SharedState>,
    Json(req): Json<event::VerifyEventRequest>,
) -> Result<impl IntoResponse> {
    let severity = if req.failed > 0 {
        EventSeverity::Warning
    } else {
        EventSeverity::Info
    };
    let state_fb = state.clone();
    let event = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        let event = state_fb.event_bus.ingest(
            engine.graph(),
            req.project.clone(),
            EventType::TaskVerify,
            severity,
            format!("verify:{}", req.task_type),
            format!(
                "Deliverable verify: {}/{} passed for {}",
                req.passed,
                req.passed + req.failed,
                req.task_type
            ),
            serde_json::json!({
                "agent_id": req.agent_id,
                "task_type": req.task_type,
                "claimed_files": req.claimed_files,
                "passed": req.passed,
                "failed": req.failed,
                "failures": req.failures,
            }),
        )?;
        let _ = state_fb.audit_store.log_event_ingested(
            engine.graph(),
            &req.project,
            &format!("verify:{}", req.task_type),
            EventType::TaskVerify,
        );
        Ok::<_, crate::error::EnvoyError>(event)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    broadcast_to_project(
        &state,
        &event.project,
        "verify_event",
        &serde_json::to_value(&event).unwrap_or_default(),
    )
    .await;
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
    let state_fb = state.clone();
    let project = params.project.clone();
    let since = params.since.clone();
    let limit = params.limit.unwrap_or(50).min(100);
    let events = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .event_bus
            .query(engine.graph(), &project, since.as_deref(), Some(limit))
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(serde_json::json!({
        "events": events,
        "count": events.len(),
    })))
}

#[derive(Debug, serde::Deserialize)]
struct AuditQueryParams {
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    operation: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn query_audit(
    State(state): State<SharedState>,
    Query(params): Query<AuditQueryParams>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let limit = params.limit.unwrap_or(50).min(100);
    let events = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb.audit_store.query(
            engine.graph(),
            params.agent_id.as_deref(),
            params.operation.as_deref(),
            params.task_id.as_deref(),
            params.since.as_deref(),
            Some(limit),
        )
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(serde_json::json!({
        "events": events,
        "count": events.len(),
    })))
}

async fn query_task_audit(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let events = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .audit_store
            .query(engine.graph(), None, None, Some(&task_id), None, Some(50))
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
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
    let state_fb = state.clone();
    let task = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb.task_store.propose(
            engine.graph(),
            req.project.clone(),
            req.description,
            req.blocked_by,
        )
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    broadcast_to_project(
        &state,
        &task.project,
        "task_proposed",
        &serde_json::to_value(&task).unwrap_or_default(),
    )
    .await;
    Ok((axum::http::StatusCode::CREATED, Json(task)))
}

async fn claim_task(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
    Json(req): Json<task::ClaimTaskRequest>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let tid = task_id.clone();
    let task = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        let task = state_fb
            .task_store
            .claim(engine.graph(), &tid, req.agent_id.clone())?;
        let _ = state_fb
            .audit_store
            .log_task_claimed(engine.graph(), &tid, &req.agent_id);
        Ok::<_, crate::error::EnvoyError>(task)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    broadcast_to_project(
        &state,
        &task.project,
        "task_claimed",
        &serde_json::to_value(&task).unwrap_or_default(),
    )
    .await;
    Ok(Json(task))
}

async fn claim_next_task(
    State(state): State<SharedState>,
    Json(req): Json<task::ClaimNextRequest>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let task = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .task_store
            .claim_next(engine.graph(), &req.project, req.agent_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    broadcast_to_project(
        &state,
        &task.project,
        "task_claimed",
        &serde_json::to_value(&task).unwrap_or_default(),
    )
    .await;
    Ok((axum::http::StatusCode::CREATED, Json(task)))
}

async fn update_task_state(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
    Json(req): Json<task::UpdateTaskStateRequest>,
) -> Result<impl IntoResponse> {
    let new_state: TaskState = req.state.parse()?;
    let is_done = new_state == TaskState::Done;
    let state_fb = state.clone();
    let tid = task_id.clone();
    let (task, blocked) = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        let task = state_fb.task_store.update_state(
            engine.graph(),
            &tid,
            new_state,
            req.checkpoint,
            None,
        )?;
        let blocked = if is_done {
            state_fb.task_store.find_blocked_by(engine.graph(), &tid)?
        } else {
            Vec::new()
        };
        Ok::<_, EnvoyError>((task, blocked))
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;

    // WS notifications are in-memory
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
    broadcast_to_project(
        &state,
        &task.project,
        "task_state_changed",
        &serde_json::to_value(&task).unwrap_or_default(),
    )
    .await;
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
    let filter = params.state.as_deref().and_then(|s| s.parse().ok());
    let state_fb = state.clone();
    let project = params.project.clone();
    let tasks = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .task_store
            .list(engine.graph(), &project, filter.as_ref())
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(serde_json::json!({
        "tasks": tasks,
        "count": tasks.len(),
    })))
}

async fn get_task(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let task = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb.task_store.get(engine.graph(), &task_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
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
    // Verify agent exists before subscribing
    state.agent_registry.get(agent_id)?;
    let state_fb = state.clone();
    let aid = agent_id.to_string();
    let proj = project.to_string();
    tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .subscription_store
            .subscribe(engine.graph(), &aid, &proj)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({"subscribed": true, "agent_id": agent_id, "project": project})),
    ))
}

async fn unsubscribe_agent(
    State(state): State<SharedState>,
    Path((agent_id, project)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb
            .subscription_store
            .unsubscribe(engine.graph(), &agent_id, &project)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(serde_json::json!({"unsubscribed": true})))
}

async fn list_subscriptions(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let aid = agent_id.clone();
    let subs = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb.subscription_store.list(engine.graph(), &aid)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(
        serde_json::json!({"agent_id": agent_id, "subscriptions": subs}),
    ))
}

// ── Project config handlers ──

async fn get_project_config(
    State(state): State<SharedState>,
    Path(project): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let cfg = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb.project_config_store.get(engine.graph(), &project)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(cfg))
}

async fn set_project_config(
    State(state): State<SharedState>,
    Path(project): Path<String>,
    Json(cfg): Json<ProjectConfig>,
) -> Result<impl IntoResponse> {
    let mut cfg = cfg;
    cfg.project = project.clone();
    let state_fb = state.clone();
    tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock().unwrap();
        state_fb.project_config_store.set(engine.graph(), &cfg)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(
        serde_json::json!({"configured": true, "project": project}),
    ))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    {
        let state_fb = state.clone();
        let agent_id = agent_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let engine = state_fb.engine.lock().unwrap();
            state_fb.agent_registry.heartbeat(
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
            )
        })
        .await;
    }

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state, agent_id)))
}

async fn handle_ws(mut socket: WebSocket, state: SharedState, agent_id: String) {
    let mut rx = state.ws_registry.register(&agent_id);

    // Catch-up: undelivered messages for this agent
    {
        let state_fb = state.clone();
        let agent_id_fb = agent_id.clone();
        let pending = tokio::task::spawn_blocking(move || {
            let engine = state_fb.engine.lock().unwrap();
            state_fb
                .message_store
                .poll(engine.graph(), &agent_id_fb, 0, 100, true)
        })
        .await
        .unwrap_or(Ok(Vec::new()))
        .unwrap_or_default();
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

    // Catch-up: undelivered events for subscribed projects (dead-letter replay)
    let catchup_events: Vec<serde_json::Value> = {
        let state_fb = state.clone();
        let agent_id_fb = agent_id.clone();
        tokio::task::spawn_blocking(move || {
            let engine = state_fb.engine.lock().unwrap();
            let projects = state_fb
                .subscription_store
                .list(engine.graph(), &agent_id_fb)
                .unwrap_or_default();
            let mut payloads = Vec::new();
            for project in &projects {
                if let Ok(events) = state_fb.delivery_tracker.get_undelivered(
                    engine.graph(),
                    &agent_id_fb,
                    project,
                    Some(50),
                ) {
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
        })
        .await
        .unwrap_or_default()
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
    // Mark catch-up events as delivered
    if !catchup_events.is_empty() {
        let state_fb = state.clone();
        let agent_id_fb = agent_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let engine = state_fb.engine.lock().unwrap();
            for msg in &catchup_events {
                if let Some(eid) = msg
                    .get("data")
                    .and_then(|d| d.get("id"))
                    .and_then(|v| v.as_str())
                {
                    let _ = state_fb.delivery_tracker.record_delivery(
                        engine.graph(),
                        &agent_id_fb,
                        eid,
                    );
                }
            }
        })
        .await;
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
                    // Channel overflowed — replay missed messages from store
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        let _ = socket.send(Message::Text(
                            serde_json::json!({
                                "event": "channel_lagged",
                                "data": { "skipped": n }
                            }).to_string().into()
                        )).await;

                        // Replay unACKed messages from persistent store
                        let state_fb = state.clone();
                        let agent_id_fb = agent_id.clone();
                        let replay = tokio::task::spawn_blocking(move || {
                            let engine = state_fb.engine.lock().unwrap();
                            state_fb.message_store.poll(engine.graph(), &agent_id_fb, 0, 100, false)
                        })
                        .await
                        .unwrap_or(Ok(Vec::new()))
                        .unwrap_or_default();

                        for msg in &replay {
                            let event = serde_json::json!({"event": "message", "data": msg});
                            if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                                state.ws_registry.unregister(&agent_id);
                                return;
                            }
                        }

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
                                    let mut status: Option<crate::status::AgentStatusSnapshot> = None;
                                    if let Some(data) = hb.get("data") {
                                        status = serde_json::from_value::<crate::status::AgentStatusSnapshot>(data.clone()).ok();
                                    }
                                    let state_fb = state.clone();
                                    let agent_id_fb = agent_id.clone();
                                    let accepted = tokio::task::spawn_blocking(move || {
                                        let engine = state_fb.engine.lock().unwrap();
                                        if let Some(ref st) = status {
                                            state_fb.agent_registry.heartbeat(engine.graph(), &agent_id_fb, st.clone()).is_ok()
                                        } else {
                                            state_fb.agent_registry.heartbeat(engine.graph(), &agent_id_fb,
                                                crate::status::AgentStatusSnapshot::default()).is_ok()
                                        }
                                    })
                                    .await
                                    .unwrap_or(false);
                                    let _ = socket.send(Message::Text(
                                        serde_json::json!({
                                            "type": "heartbeat_ack",
                                            "data": {
                                                "accepted": accepted,
                                                "timestamp": chrono::Utc::now().to_rfc3339(),
                                            }
                                        }).to_string().into()
                                    )).await;
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
async fn broadcast_to_project(
    state: &SharedState,
    project: &str,
    event_type: &str,
    data: &serde_json::Value,
) {
    // Fetch subscribers via blocking pool
    let state_c = state.clone();
    let project_owned = project.to_string();
    let subs = match tokio::task::spawn_blocking(move || {
        let engine = state_c.engine.lock().unwrap();
        state_c
            .subscription_store
            .subscribers(engine.graph(), &project_owned)
            .unwrap_or_default()
    })
    .await
    {
        Ok(s) => s,
        Err(_) => return,
    };

    let event_id = data.get("id").and_then(|v| v.as_str());
    let mut delivery_pairs: Vec<(String, String)> = Vec::new();
    let mut offline_agents: Vec<String> = Vec::new();

    // WS sends are in-memory — safe on async runtime
    for agent_id in &subs {
        match state.circuit_breaker.check(agent_id) {
            circuit::CanDeliver::No => continue,
            circuit::CanDeliver::Yes | circuit::CanDeliver::Probe => {}
        }
        let delivered = state.ws_registry.send_json(agent_id, event_type, data);
        if delivered {
            state.circuit_breaker.record_success(agent_id);
            let state_fb = state.clone();
            let agent_id_fb = agent_id.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let engine = state_fb.engine.lock().unwrap();
                state_fb
                    .audit_store
                    .log_circuit_closed(engine.graph(), &agent_id_fb)
            })
            .await;
            if let Some(eid) = event_id {
                delivery_pairs.push((agent_id.clone(), eid.to_string()));
            }
        } else {
            state.circuit_breaker.record_failure(agent_id);
            let status = state.circuit_breaker.get_state(agent_id);
            if status.state == "open" {
                let state_fb = state.clone();
                let agent_id_fb = agent_id.clone();
                let failures = status.failures;
                let _ = tokio::task::spawn_blocking(move || {
                    let engine = state_fb.engine.lock().unwrap();
                    state_fb
                        .audit_store
                        .log_circuit_opened(engine.graph(), &agent_id_fb, failures)
                })
                .await;
            }
            offline_agents.push(agent_id.clone());
        }
    }

    // Record deliveries via blocking pool
    if !delivery_pairs.is_empty() {
        let state_c = state.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let engine = state_c.engine.lock().unwrap();
            for (agent_id, eid) in &delivery_pairs {
                let _ = state_c
                    .delivery_tracker
                    .record_delivery(engine.graph(), agent_id, eid);
            }
        })
        .await;
    }

    // Store notifications for offline agents so they pick them up on poll/reconnect
    if !offline_agents.is_empty() {
        let state_c = state.clone();
        let event_type_owned = event_type.to_string();
        let data_clone = data.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let engine = state_c.engine.lock().unwrap();
            for agent_id in &offline_agents {
                let _ = state_c.message_store.store_notification(
                    engine.graph(),
                    agent_id,
                    &event_type_owned,
                    &data_clone,
                );
            }
        })
        .await;
    }
}
