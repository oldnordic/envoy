use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;

use crate::error::{EnvoyError, Result};
use crate::http::state::{recover_lock, SharedState};
// ── Subscription handlers ──

pub(crate) async fn subscribe_agent(
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
        let engine = recover_lock(&state_fb.engine);
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

pub(crate) async fn unsubscribe_agent(
    State(state): State<SharedState>,
    Path((agent_id, project)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
        state_fb
            .subscription_store
            .unsubscribe(engine.graph(), &agent_id, &project)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(serde_json::json!({"unsubscribed": true})))
}

pub(crate) async fn list_subscriptions(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let aid = agent_id.clone();
    let subs = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
        state_fb.subscription_store.list(engine.graph(), &aid)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(
        serde_json::json!({"agent_id": agent_id, "subscriptions": subs}),
    ))
}
