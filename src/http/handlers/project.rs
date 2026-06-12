use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;

use crate::error::{EnvoyError, Result};
use crate::http::state::SharedState;
use crate::monitor::ProjectConfig;
// ── Project config handlers ──

pub(crate) async fn get_project_config(
    State(state): State<SharedState>,
    Path(project): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let cfg = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
        state_fb.project_config_store.get(engine.graph(), &project)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(cfg))
}

pub(crate) async fn set_project_config(
    State(state): State<SharedState>,
    Path(project): Path<String>,
    Json(cfg): Json<ProjectConfig>,
) -> Result<impl IntoResponse> {
    let mut cfg = cfg;
    cfg.project = project.clone();
    let state_fb = state.clone();
    tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
        state_fb.project_config_store.set(engine.graph(), &cfg)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(
        serde_json::json!({"configured": true, "project": project}),
    ))
}
