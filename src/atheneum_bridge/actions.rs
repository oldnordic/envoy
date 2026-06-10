use axum::extract::{Query, State};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::atheneum_bridge::types::*;
use crate::atheneum_bridge::utils::entity_to_json;
use crate::error::Result;
use crate::http::AppState;

pub async fn post_action(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateActionRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let trace = state
        .with_atheneum_async(move |g| {
            use atheneum::graph::ToolCallRecord;
            let tool_calls: Vec<ToolCallRecord> = req
                .tool_calls
                .into_iter()
                .map(|tc| ToolCallRecord {
                    tool_name: tc.tool_name,
                    args: tc.args,
                    modified_targets: tc.modified_targets,
                })
                .collect();
            g.record_agent_action(
                &req.agent,
                &req.thought,
                tool_calls,
                req.project_id.as_deref(),
            )
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ActionTraceResponse {
            agent_id: trace.agent_id,
            reasoning_log_id: trace.reasoning_log_id,
            tool_call_ids: trace.tool_call_ids,
            modified_edge_ids: trace.modified_edge_ids,
        }),
    ))
}

/// GET /atheneum/actions?agent=X[&project=Y] — get_action_trace over HTTP.
pub async fn get_actions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GetActionsQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let agent = query.agent.clone();
    let project = query.project.clone();

    let actions: Vec<serde_json::Value> = state
        .with_atheneum_async(move |g| {
            let records = g
                .get_action_trace(&agent, project.as_deref())
                .map_err(crate::error::EnvoyError::from)?;
            Ok(records
                .into_iter()
                .map(|r| {
                    json!({
                        "reasoning_log": entity_to_json(r.reasoning_log),
                        "tool_calls": r
                            .tool_calls
                            .into_iter()
                            .map(|tc| json!({
                                "tool_call": entity_to_json(tc.tool_call),
                                "modified": tc.modified.into_iter().map(entity_to_json).collect::<Vec<_>>(),
                            }))
                            .collect::<Vec<_>>(),
                    })
                })
                .collect())
        })
        .await?;

    Ok(Json(GetActionsResponse { actions }))
}
