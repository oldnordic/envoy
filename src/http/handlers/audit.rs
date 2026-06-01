use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::error::{EnvoyError, Result};
use crate::event::{self, EventSeverity, EventType};
use crate::http::state::{recover_lock, SharedState};
use crate::http::ws::broadcast_to_project;
// ── Event handlers ──

pub(crate) async fn ingest_hook_event(
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
        let engine = recover_lock(&state_fb.engine);
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

pub(crate) async fn ingest_gate_event(
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
        let engine = recover_lock(&state_fb.engine);
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

pub(crate) async fn ingest_ci_event(
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
        let engine = recover_lock(&state_fb.engine);
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

pub(crate) async fn ingest_doc_event(
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
        let engine = recover_lock(&state_fb.engine);
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

pub(crate) async fn ingest_verify_event(
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
        let engine = recover_lock(&state_fb.engine);
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
pub struct EventQueryParams {
    project: String,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

pub(crate) async fn query_events(
    State(state): State<SharedState>,
    Query(params): Query<EventQueryParams>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let project = params.project.clone();
    let since = params.since.clone();
    let limit = params.limit.unwrap_or(50).min(100);
    let events = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
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
pub struct AuditQueryParams {
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

pub(crate) async fn query_audit(
    State(state): State<SharedState>,
    Query(params): Query<AuditQueryParams>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let limit = params.limit.unwrap_or(50).min(100);
    let events = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
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

pub(crate) async fn query_task_audit(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let events = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
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
