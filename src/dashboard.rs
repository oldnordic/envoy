//! Dashboard API handlers
//!
//! Provides graph, kanban, and audit data for the web dashboard.

use axum::extract::{Query, State, WebSocketUpgrade};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::broadcast;

use crate::error::Result;
use crate::http::SharedState;

/// Graph node for visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub data: serde_json::Value,
}

/// Graph edge for visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub data: serde_json::Value,
}

/// Dashboard statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardStats {
    pub agents: usize,
    pub discoveries: usize,
    pub edges: usize,
    pub discovery_types: HashMap<String, usize>,
}

/// Audit event for timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub timestamp: String,
    pub event_type: String,
    pub agent: String,
    pub source: String,
    pub data: serde_json::Value,
}

/// Task for kanban view
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanTask {
    pub id: String,
    pub description: String,
    pub state: String,
    pub claimed_by: Option<String>,
}

/// Get all graph nodes from atheneum
pub async fn get_graph_nodes_impl(state: &SharedState) -> Result<Vec<GraphNode>> {
    let atheneum_path = state.atheneum_path.clone();

    state
        .with_engine_async(move |_engine| {
            let mut nodes = Vec::new();

            #[cfg(feature = "atheneum")]
            if let Some(path) = atheneum_path {
                use atheneum::graph::AtheneumGraph;
                if let Ok(atheneum) = AtheneumGraph::open(std::path::Path::new(&path)) {
                    // Get agents
                    if let Ok(agents) = atheneum.entities_by_kind("Agent") {
                        for agent in agents {
                            nodes.push(GraphNode {
                                id: agent.id.to_string(),
                                kind: "Agent".to_string(),
                                label: agent.name.clone(),
                                data: agent.data,
                            });
                        }
                    }

                    // Get discoveries
                    if let Ok(discoveries) = atheneum.entities_by_kind("Discovery") {
                        for discovery in discoveries {
                            nodes.push(GraphNode {
                                id: discovery.id.to_string(),
                                kind: "Discovery".to_string(),
                                label: discovery.name.clone(),
                                data: discovery.data,
                            });
                        }
                    }

                    // Get events
                    if let Ok(events) = atheneum.entities_by_kind("Event") {
                        for event in events {
                            nodes.push(GraphNode {
                                id: event.id.to_string(),
                                kind: "Event".to_string(),
                                label: event.name.clone(),
                                data: event.data,
                            });
                        }
                    }
                }
            }

            Ok(nodes)
        })
        .await
}

/// Get all graph edges from atheneum
pub async fn get_graph_edges_impl(state: &SharedState) -> Result<Vec<GraphEdge>> {
    let atheneum_path = state.atheneum_path.clone();

    state
        .with_engine_async(move |_engine| {
            let mut edges = Vec::new();

            #[cfg(feature = "atheneum")]
            if let Some(path) = atheneum_path {
                use atheneum::graph::AtheneumGraph;
                if let Ok(atheneum) = AtheneumGraph::open(std::path::Path::new(&path)) {
                    // FIX: Query ALL edges, not just agent outgoing
                    // The bug was querying outgoing from Agent, but edges are Event→Agent
                    // Solution: Query all entities and get ALL edges (both incoming and outgoing)
                    if let Ok(agents) = atheneum.entities_by_kind("Agent") {
                        for agent in agents {
                            // Get incoming edges (Event → Agent)
                            if let Ok(incoming) = atheneum.incoming_edges(agent.id) {
                                for edge in incoming {
                                    edges.push(GraphEdge {
                                        id: edge.id.to_string(),
                                        source: edge.from_id.to_string(),
                                        target: edge.to_id.to_string(),
                                        kind: edge.edge_type.clone(),
                                        data: edge.data,
                                    });
                                }
                            }
                            // Also get outgoing edges (Agent → Discovery/other)
                            if let Ok(outgoing) = atheneum.outgoing_edges(agent.id) {
                                for edge in outgoing {
                                    edges.push(GraphEdge {
                                        id: edge.id.to_string(),
                                        source: edge.from_id.to_string(),
                                        target: edge.to_id.to_string(),
                                        kind: edge.edge_type.clone(),
                                        data: edge.data,
                                    });
                                }
                            }
                        }
                    }

                    // Also get edges from Events (some events might not be connected to agents yet)
                    if let Ok(events) = atheneum.entities_by_kind("Event") {
                        for event in events {
                            if let Ok(outgoing) = atheneum.outgoing_edges(event.id) {
                                for edge in outgoing {
                                    // Avoid duplicates by checking if we already have this edge
                                    if !edges.iter().any(|e| e.id == edge.id.to_string()) {
                                        edges.push(GraphEdge {
                                            id: edge.id.to_string(),
                                            source: edge.from_id.to_string(),
                                            target: edge.to_id.to_string(),
                                            kind: edge.edge_type.clone(),
                                            data: edge.data,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Ok(edges)
        })
        .await
}

/// Get dashboard statistics
pub async fn get_graph_stats_impl(state: &SharedState) -> Result<DashboardStats> {
    let atheneum_path = state.atheneum_path.clone();

    state
        .with_engine_async(move |_engine| {
            let mut stats = DashboardStats {
                agents: 0,
                discoveries: 0,
                edges: 0,
                discovery_types: HashMap::new(),
            };

            #[cfg(feature = "atheneum")]
            if let Some(path) = atheneum_path {
                use atheneum::graph::AtheneumGraph;
                if let Ok(atheneum) = AtheneumGraph::open(std::path::Path::new(&path)) {
                    // Count entities by kind
                    if let Ok(counts) = atheneum.count_entities_by_kind() {
                        for (kind, count) in counts {
                            match kind.as_str() {
                                "Agent" => stats.agents = count as usize,
                                "Discovery" => stats.discoveries = count as usize,
                                _ => {}
                            }
                        }
                    }

                    // Count edges by type
                    if let Ok(edge_counts) = atheneum.count_edges_by_type() {
                        for (_edge_type, count) in edge_counts {
                            stats.edges += count as usize;
                        }
                    }

                    // Count discovery types
                    if let Ok(discoveries) = atheneum.entities_by_kind("Discovery") {
                        for discovery in discoveries {
                            if let Some(discovery_type) = discovery
                                .data
                                .get("discovery_type")
                                .and_then(|v| v.as_str())
                            {
                                *stats
                                    .discovery_types
                                    .entry(discovery_type.to_string())
                                    .or_insert(0) += 1;
                            }
                        }
                    }
                }
            }

            Ok(stats)
        })
        .await
}

/// Get tasks grouped by state for kanban view
pub async fn get_dashboard_tasks_impl(
    state: &SharedState,
) -> Result<HashMap<String, Vec<KanbanTask>>> {
    let state_clone = state.clone();

    state
        .with_engine_async(move |engine| {
            let mut grouped = HashMap::new();

            // List tasks from main projects
            let projects = ["kanban", "envoy", "magellan", "general", ""];
            for project in projects {
                let tasks = state_clone
                    .task_store
                    .list(engine.graph(), project, None)
                    .unwrap_or_default();

                for task in tasks {
                    // Map TaskState to kanban columns
                    let kanban_column = match task.state {
                        crate::task::TaskState::Proposed | crate::task::TaskState::Claimed => {
                            "TODO"
                        }
                        crate::task::TaskState::InProgress
                        | crate::task::TaskState::WaitingReview => "IN_PROGRESS",
                        crate::task::TaskState::Done => "DONE",
                    };

                    let kanban_task = KanbanTask {
                        id: task.id,
                        description: task.description.clone(),
                        state: kanban_column.to_string(),
                        claimed_by: task.claimed_by.clone(),
                    };
                    grouped
                        .entry(kanban_column.to_string())
                        .or_insert_with(Vec::new)
                        .push(kanban_task);
                }
            }

            Ok(grouped)
        })
        .await
}

/// Get audit timeline
pub async fn get_dashboard_audit_impl(
    state: &SharedState,
    limit: Option<usize>,
    since: Option<String>,
) -> Result<Vec<AuditEvent>> {
    let state_clone = state.clone();
    let since_clone = since.clone();
    let limit_i64 = limit.map(|l| l as i64);

    state
        .with_engine_async(move |engine| {
            let mut events = Vec::new();

            let audit_events = state_clone
                .audit_store
                .query(
                    engine.graph(),
                    None, // agent_id
                    None, // operation
                    None, // task_id
                    since_clone.as_deref(),
                    limit_i64,
                )
                .unwrap_or_default();

            for event in audit_events {
                events.push(AuditEvent {
                    id: event.id.to_string(),
                    timestamp: event.timestamp.to_rfc3339(),
                    event_type: format!("{:?}", event.event_type),
                    agent: event
                        .data
                        .get("agent_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    source: event.source.clone(),
                    data: event.data,
                });
            }

            Ok(events)
        })
        .await
}

// ============================================================================
// HTTP Route Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub limit: Option<usize>,
    pub since: Option<String>,
}

/// GET /api/dashboard/graph/nodes
pub async fn get_graph_nodes(State(state): State<SharedState>) -> Result<Json<Vec<GraphNode>>> {
    let nodes = get_graph_nodes_impl(&state).await?;
    Ok(Json(nodes))
}

/// GET /api/dashboard/graph/edges
pub async fn get_graph_edges(State(state): State<SharedState>) -> Result<Json<Vec<GraphEdge>>> {
    let edges = get_graph_edges_impl(&state).await?;
    Ok(Json(edges))
}

/// GET /api/dashboard/graph/stats
pub async fn get_graph_stats(State(state): State<SharedState>) -> Result<Json<DashboardStats>> {
    let stats = get_graph_stats_impl(&state).await?;
    Ok(Json(stats))
}

/// GET /api/dashboard/tasks
pub async fn get_dashboard_tasks(
    State(state): State<SharedState>,
) -> Result<Json<HashMap<String, Vec<KanbanTask>>>> {
    let tasks = get_dashboard_tasks_impl(&state).await?;
    Ok(Json(tasks))
}

/// GET /api/dashboard/audit
pub async fn get_dashboard_audit(
    State(state): State<SharedState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEvent>>> {
    let events = get_dashboard_audit_impl(&state, query.limit, query.since).await?;
    Ok(Json(events))
}

/// Add dashboard routes to the router
pub fn add_dashboard_routes(routes: Router<SharedState>) -> Router<SharedState> {
    routes
        .route("/api/dashboard/graph/nodes", get(get_graph_nodes))
        .route("/api/dashboard/graph/edges", get(get_graph_edges))
        .route("/api/dashboard/graph/stats", get(get_graph_stats))
        .route("/api/dashboard/tasks", get(get_dashboard_tasks))
        .route("/api/dashboard/audit", get(get_dashboard_audit))
        .route("/api/dashboard/stream", get(dashboard_ws_handler))
}

// ============================================================================
// Dashboard WebSocket Registry
// ============================================================================

/// Registry for dashboard WebSocket connections.
/// Broadcasts events to all connected dashboard clients.
#[derive(Clone)]
pub struct DashboardWsRegistry {
    sender: broadcast::Sender<String>,
}

impl DashboardWsRegistry {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(100);
        Self { sender }
    }

    /// Subscribe to dashboard events
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }

    /// Broadcast an event to all dashboard clients
    pub fn broadcast(&self, event: &serde_json::Value) -> Result<()> {
        let event_str = event.to_string();
        self.sender
            .send(event_str)
            .map_err(|e| crate::error::EnvoyError::WsError(e.to_string()))?;
        Ok(())
    }
}

impl Default for DashboardWsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Dashboard WebSocket Handler
// ============================================================================

/// WebSocket upgrade handler for dashboard live updates
pub async fn dashboard_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(|socket| handle_dashboard_ws(socket, state))
}

/// Handle dashboard WebSocket connection
async fn handle_dashboard_ws(mut socket: axum::extract::ws::WebSocket, state: SharedState) {
    // Subscribe to dashboard events
    let mut rx = state.dashboard_ws_registry.subscribe();

    // Send initial connection message
    let welcome_msg = serde_json::json!({
        "event": "connected",
        "data": {
            "message": "Dashboard WebSocket connected",
            "timestamp": chrono::Utc::now().to_rfc3339()
        }
    });

    if socket
        .send(axum::extract::ws::Message::Text(
            welcome_msg.to_string().into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    // Use tokio::select to handle both sending and receiving
    loop {
        tokio::select! {
            // Broadcast events to client
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if socket.send(axum::extract::ws::Message::Text(event.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            // Handle incoming messages (ping/pong/close)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(axum::extract::ws::Message::Ping(_))) => {}
                    Some(Ok(axum::extract::ws::Message::Pong(_))) => {}
                    Some(Ok(axum::extract::ws::Message::Close(_))) => break,
                    Some(Err(_)) => break,
                    None => break,
                    _ => {}
                }
            }
        }
    }
}
