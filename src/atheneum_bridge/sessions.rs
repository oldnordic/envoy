use axum::extract::{Path, Query, State};
use axum::Json;
use std::sync::Arc;

use crate::atheneum_bridge::types::*;
use crate::error::Result;
use crate::http::AppState;
use atheneum::graph::SessionSummary;

pub async fn post_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordSessionRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.require_atheneum_path()?;
    let session_id = req.session_id.clone();
    let session_id_for_response = session_id.clone();
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.record_session(atheneum::graph::SessionParams {
                session_id: req.session_id,
                agent_name: req.agent,
                project: req.project,
                tool: req.tool,
                trigger: req.trigger,
                model: req.model,
                git_branch: req.git_branch,
                git_head: req.git_head,
                parent_session_id: req.parent_session_id,
            })
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(RecordSessionResponse {
            session_id: session_id_for_response,
            recorded: true,
        }),
    ))
}

/// PATCH /atheneum/sessions/{id} — end a session
pub async fn patch_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(req): Json<EndSessionRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.require_atheneum_path()?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.end_session(atheneum::graph::EndSessionParams {
                session_id,
                exit_status: req.exit_status,
                prompt_count: req.prompt_count as i64,
                tool_call_count: req.tool_call_count as i64,
                file_write_count: req.file_write_count as i64,
                commit_count: req.commit_count as i64,
                test_run_count: req.test_run_count as i64,
                total_input_tokens: req.total_input_tokens as i64,
                total_output_tokens: req.total_output_tokens as i64,
                total_cost_usd: req.total_cost_usd,
            })
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::OK)
}

/// POST /atheneum/prompts — record a prompt
pub async fn post_prompt(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordPromptRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.require_atheneum_path()?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.record_evidence_prompt(atheneum::graph::PromptParams {
                session_id: req.session_id,
                role: req.role,
                sequence: req.sequence as i64,
                input_hash: req.input_hash,
                input_tokens: req.input_tokens.map(|v| v as i64),
                output_hash: req.output_hash,
                output_tokens: req.output_tokens.map(|v| v as i64),
                latency_ms: req.latency_ms.map(|v| v as i64),
                model: req.model,
                cost_usd: req.cost_usd,
            })
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::CREATED)
}

/// POST /atheneum/tool-calls — record a tool call
pub async fn post_tool_call(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordToolCallRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.require_atheneum_path()?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.record_evidence_tool_call(atheneum::graph::ToolCallParams {
                session_id: req.session_id,
                tool_name: req.tool_name,
                tool_version: req.tool_version,
                input_hash: req.input_hash,
                input_summary: req.input_summary,
                output_hash: req.output_hash,
                output_summary: req.output_summary,
                exit_status: req.exit_status,
                latency_ms: req.latency_ms as i64,
                input_tokens_est: req.input_tokens_est.map(|v| v as i64),
                tool_category: req.tool_category,
            })
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::CREATED)
}

/// POST /atheneum/file-writes — record a file write
pub async fn post_file_write(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordFileWriteRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.require_atheneum_path()?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.record_evidence_file_write(atheneum::graph::FileWriteParams {
                session_id: req.session_id,
                file_path: req.file_path,
                file_id: req.file_id,
                before_hash: req.before_hash,
                after_hash: req.after_hash,
                lines_added: req.lines_added as i64,
                lines_deleted: req.lines_deleted as i64,
                lines_changed: req.lines_changed as i64,
                write_type: req.write_type,
            })
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::CREATED)
}

/// POST /atheneum/commits — record a commit
pub async fn post_commit(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordCommitRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.require_atheneum_path()?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.record_evidence_commit(atheneum::graph::CommitParams {
                session_id: req.session_id,
                commit_sha: req.commit_sha,
                parent_sha: req.parent_sha,
                message: req.message,
                author: req.author,
                files_changed: req.files_changed as i64,
                lines_inserted: req.lines_inserted as i64,
                lines_deleted: req.lines_deleted as i64,
                commit_type: req.commit_type,
                feature_tag: req.feature_tag,
            })
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::CREATED)
}

/// POST /atheneum/test-runs — record a test run
pub async fn post_test_run(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordTestRunRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.require_atheneum_path()?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.record_evidence_test_run(atheneum::graph::TestRunParams {
                session_id: req.session_id,
                test_name: req.test_name,
                test_suite: req.test_suite,
                test_command: req.test_command,
                result: req.result,
                duration_ms: req.duration_ms as i64,
                logs_summary: req.logs_summary,
                commit_sha: req.commit_sha,
            })
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::CREATED)
}

/// POST /atheneum/fix-chains — record a fix chain
pub async fn post_fix_chain(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordFixChainRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.require_atheneum_path()?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.record_evidence_fix_chain(atheneum::graph::FixChainParams {
                session_id: req.session_id,
                bug_commit_sha: req.bug_commit_sha,
                fix_commit_sha: req.fix_commit_sha,
                fix_type: req.fix_type,
                severity: req.severity,
                cycles_to_fix: req.cycles_to_fix as i64,
                time_to_fix_ms: req.time_to_fix_ms as i64,
            })
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::CREATED)
}

/// POST /atheneum/bench-runs — record a benchmark run
pub async fn post_bench_run(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordBenchRunRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.require_atheneum_path()?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.record_evidence_bench_run(
                req.session_id,
                req.bench_name,
                req.mean_ns,
                req.median_ns,
                req.p95_ns,
                req.is_regression,
            )
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::CREATED)
}

/// GET /atheneum/events — query the event log
pub async fn get_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<QueryEventsQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.require_atheneum_path()?;
    let session_id = query.session_id.clone();
    let event_type = query.event_type.clone();
    let limit = query.limit;

    let events: Vec<serde_json::Value> = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.query_events(session_id.as_deref(), event_type.as_deref(), limit)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;

    Ok(Json(QueryEventsResponse { events }))
}

/// GET /atheneum/sessions — query recent sessions for a project
pub async fn get_sessions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<QuerySessionsQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let atheneum_path = state.require_atheneum_path()?;
    let project = query.project.clone();
    let last = query.last;
    let parent_id = query.parent_id.clone();

    let sessions: Vec<SessionSummary> = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.query_sessions(&project, last, parent_id.as_deref())
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;

    Ok(Json(sessions))
}

/// POST /atheneum/sessions/{id}/handover — record subagent handover note
pub async fn post_subagent_handover(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(req): Json<SubagentHandoverRequest>,
) -> Result<axum::http::StatusCode> {
    let atheneum_path = state.require_atheneum_path()?;
    state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.record_subagent_handover(&session_id, &req.summary, &req.files_changed, &req.outcome)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(axum::http::StatusCode::CREATED)
}
