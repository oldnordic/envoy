use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use tokio::sync::broadcast;

use crate::circuit;
use crate::error::Result;
use crate::http::state::SharedState;

pub(crate) async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse> {
    {
        let state_fb = state.clone();
        let agent_id = agent_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let engine = state_fb.engine.lock();
            state_fb.agent_registry.heartbeat(
                engine.graph(),
                &agent_id,
                crate::status::AgentStatusSnapshot {
                    state: crate::status::AgentState::Working,
                    task_id: None,
                    blocked_reason: None,
                    waiting_on_agent: None,
                    checkpoint: Some("ws_connected".into()),
                    working_on: "connected via WS".into(),
                },
            )
        })
        .await;
    }

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state, agent_id)))
}

async fn handle_ws(mut socket: WebSocket, state: SharedState, agent_id: String) {
    let mut rx = state.ws_registry.register(&agent_id);

    // Catch-up: undelivered messages for this agent
    {
        let state_fb = state.clone();
        let agent_id_fb = agent_id.clone();
        let pending = tokio::task::spawn_blocking(move || {
            let engine = state_fb.engine.lock();
            state_fb
                .message_store
                .poll(engine.graph(), &agent_id_fb, 0, 100, true)
        })
        .await
        .unwrap_or(Ok(Vec::new()))
        .unwrap_or_default();
        for msg in &pending {
            let event = serde_json::json!({"event": "message", "data": msg});
            if socket
                .send(Message::Text(event.to_string().into()))
                .await
                .is_err()
            {
                state.ws_registry.unregister(&agent_id);
                return;
            }
        }
    }

    // Catch-up: undelivered events for subscribed projects (dead-letter replay)
    let catchup_events: Vec<serde_json::Value> = {
        let state_fb = state.clone();
        let agent_id_fb = agent_id.clone();
        tokio::task::spawn_blocking(move || {
            let engine = state_fb.engine.lock();
            let projects = state_fb
                .subscription_store
                .list(engine.graph(), &agent_id_fb)
                .unwrap_or_default();
            let mut payloads = Vec::new();
            for project in &projects {
                if let Ok(events) = state_fb.delivery_tracker.get_undelivered(
                    engine.graph(),
                    &agent_id_fb,
                    project,
                    Some(50),
                ) {
                    for evt in &events {
                        if let Ok(payload) = serde_json::to_value(evt) {
                            payloads.push(
                                serde_json::json!({"event": "event_catchup", "data": payload}),
                            );
                        }
                    }
                }
            }
            payloads
        })
        .await
        .unwrap_or_default()
    };
    for msg in &catchup_events {
        if socket
            .send(Message::Text(msg.to_string().into()))
            .await
            .is_err()
        {
            state.ws_registry.unregister(&agent_id);
            return;
        }
    }
    // Mark catch-up events as delivered
    if !catchup_events.is_empty() {
        let state_fb = state.clone();
        let agent_id_fb = agent_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let engine = state_fb.engine.lock();
            for msg in &catchup_events {
                if let Some(eid) = msg
                    .get("data")
                    .and_then(|d| d.get("id"))
                    .and_then(|v| v.as_str())
                {
                    let _ = state_fb.delivery_tracker.record_delivery(
                        engine.graph(),
                        &agent_id_fb,
                        eid,
                    );
                }
            }
        })
        .await;
    }

    // Connected event
    let connected = serde_json::json!({
        "event": "agent_connected",
        "data": { "agent_id": &agent_id }
    });
    let _ = socket
        .send(Message::Text(connected.to_string().into()))
        .await;

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event_str) => {
                        if socket.send(Message::Text(event_str.into())).await.is_err() {
                            break;
                        }
                    }
                    // Channel overflowed — replay missed messages from store
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        let _ = socket.send(Message::Text(
                            serde_json::json!({
                                "event": "channel_lagged",
                                "data": { "skipped": n }
                            }).to_string().into()
                        )).await;

                        // Replay unACKed messages from persistent store
                        let state_fb = state.clone();
                        let agent_id_fb = agent_id.clone();
                        let replay = tokio::task::spawn_blocking(move || {
                            let engine = state_fb.engine.lock();
                            state_fb.message_store.poll(engine.graph(), &agent_id_fb, 0, 100, false)
                        })
                        .await
                        .unwrap_or(Ok(Vec::new()))
                        .unwrap_or_default();

                        for msg in &replay {
                            let event = serde_json::json!({"event": "message", "data": msg});
                            if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                                state.ws_registry.unregister(&agent_id);
                                return;
                            }
                        }

                        rx = state.ws_registry.register(&agent_id);
                    }
                    Err(_) => break, // channel closed
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(hb) = serde_json::from_str::<serde_json::Value>(&text) {
                            match hb.get("type").and_then(|v| v.as_str()) {
                                Some("heartbeat") => {
                                    let mut status: Option<crate::status::AgentStatusSnapshot> = None;
                                    if let Some(data) = hb.get("data") {
                                        status = serde_json::from_value::<crate::status::AgentStatusSnapshot>(data.clone()).ok();
                                    }
                                    let state_fb = state.clone();
                                    let agent_id_fb = agent_id.clone();
                                    let accepted = tokio::task::spawn_blocking(move || {
                                        let engine = state_fb.engine.lock();
                                        if let Some(ref st) = status {
                                            state_fb.agent_registry.heartbeat(engine.graph(), &agent_id_fb, st.clone()).is_ok()
                                        } else {
                                            state_fb.agent_registry.heartbeat(engine.graph(), &agent_id_fb,
                                                crate::status::AgentStatusSnapshot::default()).is_ok()
                                        }
                                    })
                                    .await
                                    .unwrap_or(false);
                                    let _ = socket.send(Message::Text(
                                        serde_json::json!({
                                            "type": "heartbeat_ack",
                                            "data": {
                                                "accepted": accepted,
                                                "timestamp": chrono::Utc::now().to_rfc3339(),
                                            }
                                        }).to_string().into()
                                    )).await;
                                    continue;
                                }
                                Some("ping") => {
                                    let _ = socket.send(Message::Text(
                                        serde_json::json!({"type": "pong"}).to_string().into()
                                    )).await;
                                    continue;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    state.ws_registry.unregister(&agent_id);
}

/// Broadcast an event to all agents subscribed to a project.
pub(crate) async fn broadcast_to_project(
    state: &SharedState,
    project: &str,
    event_type: &str,
    data: &serde_json::Value,
) {
    // Fetch subscribers via blocking pool
    let state_c = state.clone();
    let project_owned = project.to_string();
    let subs = match tokio::task::spawn_blocking(move || {
        let engine = state_c.engine.lock();
        state_c
            .subscription_store
            .subscribers(engine.graph(), &project_owned)
            .unwrap_or_default()
    })
    .await
    {
        Ok(s) => s,
        Err(_) => return,
    };

    let event_id = data.get("id").and_then(|v| v.as_str());
    let mut delivery_pairs: Vec<(String, String)> = Vec::new();
    let mut offline_agents: Vec<String> = Vec::new();

    // WS sends are in-memory — safe on async runtime
    for agent_id in &subs {
        match state.circuit_breaker.check(agent_id) {
            circuit::CanDeliver::No => continue,
            circuit::CanDeliver::Yes | circuit::CanDeliver::Probe => {}
        }
        let delivered = state.ws_registry.send_json(agent_id, event_type, data);
        if delivered {
            state.circuit_breaker.record_success(agent_id);
            let state_fb = state.clone();
            let agent_id_fb = agent_id.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let engine = state_fb.engine.lock();
                state_fb
                    .audit_store
                    .log_circuit_closed(engine.graph(), &agent_id_fb)
            })
            .await;
            if let Some(eid) = event_id {
                delivery_pairs.push((agent_id.clone(), eid.to_string()));
            }
        } else {
            state.circuit_breaker.record_failure(agent_id);
            let status = state.circuit_breaker.get_state(agent_id);
            if status.state == "open" {
                let state_fb = state.clone();
                let agent_id_fb = agent_id.clone();
                let failures = status.failures;
                let _ = tokio::task::spawn_blocking(move || {
                    let engine = state_fb.engine.lock();
                    state_fb
                        .audit_store
                        .log_circuit_opened(engine.graph(), &agent_id_fb, failures)
                })
                .await;
            }
            offline_agents.push(agent_id.clone());
        }
    }

    // Record deliveries via blocking pool
    if !delivery_pairs.is_empty() {
        let state_c = state.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let engine = state_c.engine.lock();
            for (agent_id, eid) in &delivery_pairs {
                let _ = state_c
                    .delivery_tracker
                    .record_delivery(engine.graph(), agent_id, eid);
            }
        })
        .await;
    }

    // Store notifications for offline agents so they pick them up on poll/reconnect
    if !offline_agents.is_empty() {
        let state_c = state.clone();
        let event_type_owned = event_type.to_string();
        let data_clone = data.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let engine = state_c.engine.lock();
            for agent_id in &offline_agents {
                let _ = state_c.message_store.store_notification(
                    engine.graph(),
                    agent_id,
                    &event_type_owned,
                    &data_clone,
                );
            }
        })
        .await;
    }
}
