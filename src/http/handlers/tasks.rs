use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::error::{EnvoyError, Result};
use crate::http::state::SharedState;
use crate::http::ws::broadcast_to_project;
use crate::task::{self, TaskState};
// ── Task handlers ──

pub(crate) async fn propose_task(
    State(state): State<SharedState>,
    Json(req): Json<task::ProposeTaskRequest>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let task = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
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

pub(crate) async fn claim_task(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
    Json(req): Json<task::ClaimTaskRequest>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let tid = task_id.clone();
    let task = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
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

pub(crate) async fn claim_next_task(
    State(state): State<SharedState>,
    Json(req): Json<task::ClaimNextRequest>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let task = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
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

pub(crate) async fn update_task_state(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
    Json(req): Json<task::UpdateTaskStateRequest>,
) -> Result<impl IntoResponse> {
    let new_state: TaskState = req.state.parse()?;
    let is_done = new_state == TaskState::Done;
    let state_fb = state.clone();
    let tid = task_id.clone();
    let (task, blocked) = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
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
pub struct ListTasksQuery {
    project: String,
    #[serde(default)]
    state: Option<String>,
}

pub(crate) async fn list_tasks(
    State(state): State<SharedState>,
    Query(params): Query<ListTasksQuery>,
) -> Result<impl IntoResponse> {
    let filter = params.state.as_deref().and_then(|s| s.parse().ok());
    let state_fb = state.clone();
    let project = params.project.clone();
    let tasks = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
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

pub(crate) async fn get_task(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let task = tokio::task::spawn_blocking(move || {
        let engine = state_fb.engine.lock();
        state_fb.task_store.get(engine.graph(), &task_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(task))
}
