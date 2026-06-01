use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;

use crate::error::{EnvoyError, Result};
use crate::http::state::{recover_lock, SharedState};
use crate::http::types::*;
pub(crate) async fn health(State(state): State<SharedState>) -> impl IntoResponse {
    let uptime = (chrono::Utc::now() - state.start_time).num_seconds();
    let online = state
        .agent_registry
        .list_active()
        .map(|a| a.len())
        .unwrap_or(0);

    Json(HealthResponse {
        status: "ok",
        uptime_seconds: uptime,
        agents_online: online,
    })
}

pub(crate) async fn stats(State(state): State<SharedState>) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let total = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
        state_fb
            .message_store
            .count_all(engine.graph())
            .unwrap_or(0)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))?;
    let registered = state
        .agent_registry
        .list_all()
        .map(|a| a.len())
        .unwrap_or(0);

    Ok(Json(StatsResponse {
        messages_total: total,
        agents_registered: registered,
    }))
}
