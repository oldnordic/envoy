use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;

use crate::error::{EnvoyError, Result};
use crate::http::state::{recover_lock, SharedState};
use crate::http::types::*;

pub(crate) fn info_with_online(info: &crate::agent::AgentInfo) -> serde_json::Value {
    let mut v = serde_json::to_value(info).unwrap_or_default();
    v["online"] = serde_json::json!(info.lifecycle == crate::agent::AgentLifecycle::Active);
    v
}
pub(crate) async fn register_agent(
    State(state): State<SharedState>,
    Json(req): Json<RegisterRequest>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let info = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
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
    Ok((
        axum::http::StatusCode::CREATED,
        Json(info_with_online(&info)),
    ))
}

pub(crate) async fn disconnect_agent(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let aid = agent_id.clone();
    let affected = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
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

pub(crate) async fn retire_agent(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let aid = agent_id.clone();
    let affected = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
        let affected = state_fb.agent_registry.retire(engine.graph(), &aid)?;
        let _ = state_fb
            .audit_store
            .log_agent_disconnected(engine.graph(), &aid);
        Ok::<_, crate::error::EnvoyError>(affected)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    state.circuit_breaker.remove(&agent_id);
    Ok(Json(
        serde_json::json!({"retired": true, "affected": affected}),
    ))
}

pub(crate) async fn list_agents(State(state): State<SharedState>) -> Result<impl IntoResponse> {
    let agents = state.agent_registry.list_all()?;
    let agents_json: Vec<serde_json::Value> = agents
        .iter()
        .map(|a| {
            // M-ALLOW: AgentInfo derives Serialize with simple field types; this cannot fail
            let mut v = serde_json::to_value(a).unwrap_or_default();
            v["online"] = serde_json::json!(a.lifecycle == crate::agent::AgentLifecycle::Active);
            v
        })
        .collect();
    Ok(Json(serde_json::json!({"agents": agents_json})))
}

pub(crate) async fn get_agent(
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
        "online": info.lifecycle == crate::agent::AgentLifecycle::Active,
        "parent_id": info.parent_id,
        "children": child_ids,
    })))
}
