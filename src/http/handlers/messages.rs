use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::circuit;
use crate::error::{EnvoyError, Result};
use crate::http::state::{recover_lock, SharedState};
use crate::http::types::*;
pub(crate) async fn pending_messages(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let messages = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
        state_fb
            .message_store
            .poll(engine.graph(), &agent_id, 0, 100, true)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(serde_json::json!({
        "messages": messages,
        "count": messages.len()
    })))
}

// ============================================================================
// Atheneum Auto-Persistence Helpers (cfg-gated)
// ============================================================================

/// Extract HandoffData from message parts if present
#[cfg(feature = "atheneum")]
pub(crate) fn extract_handoff_data(
    parts: &[crate::message::Part],
) -> Option<crate::message::HandoffData> {
    use crate::message::PartContent;

    for part in parts {
        if let PartContent::Data(ref data) = part.content {
            if let Ok(handoff) = serde_json::from_value::<crate::message::HandoffData>(data.clone())
            {
                return Some(handoff);
            }
        }
    }
    None
}

/// Store a handoff to atheneum (async, non-blocking)
#[cfg(feature = "atheneum")]
pub(crate) async fn store_handoff_to_atheneum(
    atheneum_path: String,
    from: String,
    to: String,
    manifest: serde_json::Value,
) {
    tokio::task::spawn_blocking(move || {
        match atheneum::graph::AtheneumGraph::open(std::path::Path::new(&atheneum_path)) {
            Ok(atheneum) => {
                if let Err(e) = atheneum.store_handoff(&from, &to, manifest) {
                    eprintln!("Failed to store handoff in atheneum: {}", e);
                }
            }
            Err(e) => eprintln!("Failed to open atheneum at {}: {}", atheneum_path, e),
        }
    })
    .await
    .ok(); // Fire-and-forget - errors are already logged
}

pub(crate) async fn send_message(
    State(state): State<SharedState>,
    Json(req): Json<SendMessageRequest>,
) -> Result<impl IntoResponse> {
    // Verify sender exists and is active
    let sender = state.agent_registry.get(&req.from)?;
    if sender.lifecycle != crate::agent::AgentLifecycle::Active {
        return Err(EnvoyError::AgentOffline(req.from));
    }

    // Reject self-messaging
    if req.from == req.to {
        return Err(EnvoyError::InvalidMessage(
            "cannot send message to self".into(),
        ));
    }

    // Verify recipient exists (in-memory)
    let _recipient = state.agent_registry.get(&req.to)?;
    let recipient = req.to.clone();

    let state_fb = state.clone();
    let stored = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
        let stored = state_fb.message_store.store(
            engine.graph(),
            req.msg_type.clone(),
            req.from.clone(),
            req.to.clone(),
            req.task_id.clone(),
            req.context_id.clone(),
            req.parts,
        )?;
        let _ = state_fb.audit_store.log_message(
            engine.graph(),
            &stored.from,
            &stored.to,
            stored.msg_type.clone(),
            &stored.message_id,
            None,
        );
        Ok::<_, crate::error::EnvoyError>(stored)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;

    // Auto-persist handoffs to atheneum (non-blocking, fire-and-forget)
    #[cfg(feature = "atheneum")]
    if stored.msg_type == crate::message::MessageType::Handoff {
        if let Some(ref atheneum_path) = state.atheneum_path {
            let atheneum_path = atheneum_path.clone();
            let from = stored.from.clone();
            let to = stored.to.clone();
            let parts = stored.parts.clone();

            // Spawn background task - failures don't affect message delivery
            tokio::spawn(async move {
                if let Some(handoff_data) = extract_handoff_data(&parts) {
                    let manifest = serde_json::to_value(&handoff_data).unwrap_or_default();
                    store_handoff_to_atheneum(atheneum_path, from, to, manifest).await;
                }
            });
        }
    }

    // Push to recipient via WebSocket if connected (in-memory)
    let event_data = serde_json::to_value(&stored).unwrap_or_default();
    match state.circuit_breaker.check(&recipient) {
        circuit::CanDeliver::Yes | circuit::CanDeliver::Probe => {
            let delivered = state
                .ws_registry
                .send_json(&recipient, "message", &event_data);
            if delivered {
                state.circuit_breaker.record_success(&recipient);
                let state_fb = state.clone();
                let recipient_fb = recipient.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let engine = recover_lock(&state_fb.engine);
                    state_fb
                        .audit_store
                        .log_circuit_closed(engine.graph(), &recipient_fb)
                })
                .await;
            } else {
                state.circuit_breaker.record_failure(&recipient);
                let status = state.circuit_breaker.get_state(&recipient);
                if status.state == "open" {
                    let state_fb = state.clone();
                    let recipient_fb = recipient.clone();
                    let failures = status.failures;
                    let _ = tokio::task::spawn_blocking(move || {
                        let engine = recover_lock(&state_fb.engine);
                        state_fb.audit_store.log_circuit_opened(
                            engine.graph(),
                            &recipient_fb,
                            failures,
                        )
                    })
                    .await;
                }
            }
        }
        circuit::CanDeliver::No => {
            // Circuit is open — message stored but not pushed. Agent will catch up on reconnect.
        }
    }

    Ok((axum::http::StatusCode::CREATED, Json(stored)))
}

pub(crate) async fn get_message(
    State(state): State<SharedState>,
    Path(message_id): Path<String>,
) -> Result<impl IntoResponse> {
    let state_fb = state.clone();
    let msg = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
        state_fb.message_store.get(engine.graph(), &message_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    Ok(Json(msg))
}

pub(crate) async fn poll_messages(
    State(state): State<SharedState>,
    Query(query): Query<PollQuery>,
) -> Result<impl IntoResponse> {
    // Verify recipient exists (in-memory)
    let _ = state.agent_registry.get(&query.to)?;

    let since = query.since.unwrap_or(0);
    let limit = query.limit.clamp(1, 100);
    let include_acked = query.include.as_deref() == Some("acked");

    let state_fb = state.clone();
    let to = query.to.clone();
    let messages = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
        state_fb
            .message_store
            .poll(engine.graph(), &to, since, limit, include_acked)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;
    let latest_seq = messages.last().map(|m| m.sequence_id).unwrap_or(since);

    Ok(Json(PollResponse {
        messages,
        latest_sequence: latest_seq,
    }))
}

#[derive(Debug, Deserialize)]
pub struct AckRequest {
    pub agent_id: String,
}

pub(crate) async fn ack_message(
    State(state): State<SharedState>,
    Path(message_id): Path<String>,
    Json(req): Json<AckRequest>,
) -> Result<impl IntoResponse> {
    // Verify agent exists (in-memory)
    let _ = state.agent_registry.get(&req.agent_id)?;

    let state_fb = state.clone();
    let mid = message_id.clone();
    let acked = tokio::task::spawn_blocking(move || {
        let engine = recover_lock(&state_fb.engine);
        state_fb
            .message_store
            .ack(engine.graph(), &mid, &req.agent_id)
    })
    .await
    .map_err(|_| EnvoyError::InvalidEntity("blocking task join error".into()))??;

    Ok(Json(serde_json::json!({
        "message_id": message_id,
        "acked_by": acked,
    })))
}
