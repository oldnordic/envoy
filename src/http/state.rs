use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::broadcast;

use crate::agent::AgentRegistry;
use crate::circuit;
use crate::dependency::DependencyStore;
use crate::engine::Engine;
use crate::error::{EnvoyError, Result};
use crate::event::bus::{DeliveryTracker, EventBus};
use crate::message::MessageStore;
use crate::monitor::{ProjectConfigStore, SubscriptionStore};
use crate::rate_limit::{HybridRateLimiter, RateLimitConfig};
use crate::status::NudgeConfig;
use crate::task::store::TaskStore;

#[cfg(feature = "atheneum")]
use atheneum::AtheneumGraph;

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

    pub(crate) fn register(&self, agent_id: &str) -> broadcast::Receiver<String> {
        let mut senders = self.senders.lock();
        if let Some(tx) = senders.get(agent_id) {
            tx.subscribe()
        } else {
            let (tx, rx) = broadcast::channel(256);
            senders.insert(agent_id.to_string(), tx);
            rx
        }
    }

    pub(crate) fn unregister(&self, agent_id: &str) {
        let mut senders = self.senders.lock();
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
        let senders = self.senders.lock();
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
    #[cfg(feature = "atheneum")]
    pub atheneum_path: Option<String>,
    #[cfg(feature = "atheneum")]
    atheneum_graph: Arc<Mutex<Option<AtheneumGraph>>>,
}

impl AppState {
    pub fn new(engine: Engine) -> Result<Self> {
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
            #[cfg(feature = "atheneum")]
            atheneum_path: None,
            #[cfg(feature = "atheneum")]
            atheneum_graph: Arc::new(Mutex::new(None)),
        })
    }

    #[cfg(feature = "atheneum")]
    pub fn with_atheneum(mut self, path: Option<String>) -> Self {
        self.atheneum_path = path;
        self
    }

    #[cfg(feature = "atheneum")]
    pub fn require_atheneum_path(&self) -> Result<String> {
        self.atheneum_path
            .clone()
            .ok_or_else(|| EnvoyError::Atheneum(anyhow::anyhow!("atheneum not configured")))
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
            let engine = engine.lock();
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
            let engine = engine.lock();
            f(&engine)
        })
        .await
        .map_err(|_| EnvoyError::InvalidEntity("blocking task panicked".into()))?
    }

    /// Async version that provides a cached AtheneumGraph.
    /// Eliminates per-request SQLite open cost by reusing a single connection.
    #[cfg(feature = "atheneum")]
    pub async fn with_atheneum_async<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&AtheneumGraph) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let path = self.require_atheneum_path()?;
        let graph_arc = self.atheneum_graph.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = graph_arc.lock();
            if guard.is_none() {
                let g = AtheneumGraph::open(std::path::Path::new(&path))?;
                *guard = Some(g);
            }
            let g = guard.as_ref().unwrap();
            f(g)
        })
        .await
        .map_err(|_| EnvoyError::InvalidEntity("blocking task panicked".into()))?
    }
}

/// Background task that checks for stale agents and pushes nudge events.
pub async fn run_nudge_loop(state: Arc<AppState>) {
    loop {
        let interval = {
            let cfg = state.nudge_config.lock();
            cfg.check_interval_seconds
        };
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

        let threshold = state.nudge_config.lock().stale_threshold_minutes;
        let stale = match state.agent_registry.get_stale_agents(threshold) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("nudge loop: failed to get stale agents: {e}");
                continue;
            }
        };

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
                let engine = state_fb.engine.lock();
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
