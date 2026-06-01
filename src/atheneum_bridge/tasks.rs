use axum::extract::{Path, Query, State};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::atheneum_bridge::types::*;
use crate::atheneum_bridge::utils::{entity_to_json, parse_blocker_type, parse_status};
use crate::error::Result;
use crate::http::AppState;

pub async fn post_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.require_atheneum_path()?;
    let task_id = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.create_task(
                &req.title,
                req.description.as_deref(),
                req.project_id.as_deref(),
            )
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(TaskCreatedResponse {
            task_id,
            status: "TODO".to_string(),
        }),
    ))
}

pub async fn get_tasks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListTasksQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.require_atheneum_path()?;
    let project = query.project.clone();
    let status_str = query.status.clone();

    let tasks: Vec<serde_json::Value> = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let entities = match status_str {
                Some(s) => {
                    let status = parse_status(&s)?;
                    g.list_tasks_by_status(status, project.as_deref())
                        .map_err(crate::error::EnvoyError::from)?
                }
                None => {
                    let all = g
                        .entities_by_kind("Task")
                        .map_err(crate::error::EnvoyError::from)?;
                    all.into_iter()
                        .filter(|t| match &project {
                            None => true,
                            Some(pid) => {
                                t.data.get("project_id").and_then(|v| v.as_str()) == Some(pid)
                            }
                        })
                        .collect()
                }
            };
            Ok(entities.into_iter().map(entity_to_json).collect())
        })
        .await?;

    Ok(Json(ListTasksResponse { tasks }))
}

pub async fn get_task_details_route(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<i64>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.require_atheneum_path()?;
    let detail = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.get_task_with_details(task_id)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(Json(TaskDetailResponse {
        task: entity_to_json(detail.task),
        requirements: detail
            .requirements
            .into_iter()
            .map(entity_to_json)
            .collect(),
        blockers: detail.blockers.into_iter().map(entity_to_json).collect(),
    }))
}

pub async fn patch_task_status_route(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<i64>,
    Json(req): Json<UpdateTaskStatusRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.require_atheneum_path()?;
    let status = parse_status(&req.status)?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.update_task_status(task_id, status)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::OK)
}

pub async fn post_task_requirement(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<i64>,
    Json(req): Json<CreateRequirementRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.require_atheneum_path()?;
    let id = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.add_requirement(task_id, &req.statement, req.verification_method.as_deref())
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({"requirement_id": id})),
    ))
}

pub async fn post_task_blocker(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<i64>,
    Json(req): Json<CreateBlockerRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.require_atheneum_path()?;
    let blocker_type = parse_blocker_type(&req.blocker_type)?;
    let id = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.add_blocker(task_id, &req.description, blocker_type)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({"blocker_id": id})),
    ))
}

pub async fn post_journal(
    State(state): State<Arc<AppState>>,
    Json(req): Json<IngestJournalRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.require_atheneum_path()?;
    let (section_ids, applied): (Vec<i64>, Vec<serde_json::Value>) = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let ids = g
                .ingest_journal(&req.path, &req.content, req.project_id.as_deref())
                .map_err(crate::error::EnvoyError::from)?;
            let mut all_applied: Vec<serde_json::Value> = Vec::new();
            for sid in &ids {
                let applied = g
                    .apply_kanban_updates_from_journal(*sid)
                    .map_err(crate::error::EnvoyError::from)?;
                for u in applied {
                    all_applied.push(json!({
                        "task_id": u.task_id,
                        "task_title": u.task_title,
                        "previous_status": u.previous_status.as_str(),
                        "new_status": u.new_status.as_str(),
                    }));
                }
            }
            Ok((ids, all_applied))
        })
        .await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(IngestJournalResponse {
            section_ids,
            applied_kanban_updates: applied,
        }),
    ))
}
