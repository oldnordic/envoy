use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;

use crate::error::{EnvoyError, Result};
use crate::http::state::SharedState;
use crate::http::types::*;
pub(crate) async fn heartbeat(
    State(state): State<SharedState>,
    Json(req): Json<crate::status::HeartbeatRequest>,
) -> Result<impl IntoResponse> {
    let agent_id = req.agent_id.clone();
    let status = req.status.clone();

    // Offload DB work to blocking pool — clone Arc<AppState> for the closure
    let state_for_blocking = state.clone();
    let deps = tokio::task::spawn_blocking(move || {
        let engine = state_for_blocking.engine.lock();
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

pub(crate) async fn get_circuit(
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

pub(crate) async fn record_circuit_failure(
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

pub(crate) async fn create_dependency(
    State(state): State<SharedState>,
    Json(req): Json<CreateDependencyRequest>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let dep = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
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

pub(crate) async fn get_blocker_deps(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let deps = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
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

pub(crate) async fn get_dependent_deps(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let deps = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
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

pub(crate) async fn resolve_dependency(
    State(state): State<SharedState>,
    Path(dep_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let dep = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
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

pub(crate) async fn update_nudge_config(
    State(state): State<SharedState>,
    Json(cfg): Json<crate::status::NudgeConfig>,
) -> Result<impl IntoResponse> {
    let mut current = state.nudge_config.lock();
    *current = cfg.clone();
    Ok(Json(cfg))
}

pub(crate) async fn get_nudge_config(
    State(state): State<SharedState>,
) -> Result<impl IntoResponse> {
    let cfg = state.nudge_config.lock().clone();
    Ok(Json(cfg))
}
