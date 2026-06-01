use envoy::{AgentStatus, Engine, EnvoyError, EventPayload};

#[test]
fn create_and_get_channel() {
    let engine = Engine::open_in_memory().unwrap();
    let channel = engine
        .create_channel("test-channel", "A test channel")
        .unwrap();
    assert_eq!(channel.name, "test-channel");
    assert_eq!(channel.description, "A test channel");
    assert!(
        !channel.created_at.is_empty(),
        "created_at should be set on new channel"
    );
    assert!(
        channel.created_at.contains('T'),
        "created_at should be RFC 3339"
    );

    let found = engine.get_channel("test-channel").unwrap();
    assert_eq!(found.id, channel.id);
    assert_eq!(
        found.created_at, channel.created_at,
        "created_at should round-trip"
    );

    let by_id = engine.get_channel_by_id(channel.id).unwrap();
    assert_eq!(by_id.name, "test-channel");
}

#[test]
fn list_channels() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("ch1", "first").unwrap();
    engine.create_channel("ch2", "second").unwrap();

    let channels = engine.list_channels().unwrap();
    assert_eq!(channels.len(), 2);
}

#[test]
fn duplicate_channel_rejected() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("dup", "first").unwrap();
    let err = engine.create_channel("dup", "second").unwrap_err();
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn publish_and_replay_events() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("updates", "Agent updates").unwrap();

    let payload = EventPayload {
        status: AgentStatus::Working,
        working_on: "fixing bug #42".into(),
        waiting_for: None,
        can_start: None,
        verified: false,
        magellan_trace: None,
        extra: serde_json::Value::Null,
    };

    let event = engine
        .publish("updates", "claude", payload.clone())
        .unwrap();
    assert_eq!(event.channel_name, "updates");
    assert_eq!(event.sender, "claude");
    assert_eq!(event.sequence_id, 1);
    assert_eq!(event.payload.status, AgentStatus::Working);

    // Replay from 0 should return the event
    let events = engine.replay("updates", 0, None).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].sequence_id, 1);

    // Replay from 1 should return empty
    let events = engine.replay("updates", 1, None).unwrap();
    assert!(events.is_empty());
}

#[test]
fn sequence_ids_increment() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("seq-test", "").unwrap();

    let payload = EventPayload {
        status: AgentStatus::Working,
        working_on: "task".into(),
        waiting_for: None,
        can_start: None,
        verified: false,
        magellan_trace: None,
        extra: serde_json::Value::Null,
    };

    let e1 = engine
        .publish("seq-test", "agent", payload.clone())
        .unwrap();
    let e2 = engine
        .publish("seq-test", "agent", payload.clone())
        .unwrap();
    let e3 = engine.publish("seq-test", "agent", payload).unwrap();

    assert_eq!(e1.sequence_id, 1);
    assert_eq!(e2.sequence_id, 2);
    assert_eq!(e3.sequence_id, 3);
}

#[test]
fn replay_with_limit() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("limited", "").unwrap();

    let payload = EventPayload {
        status: AgentStatus::Done,
        working_on: "task".into(),
        waiting_for: None,
        can_start: None,
        verified: true,
        magellan_trace: None,
        extra: serde_json::Value::Null,
    };

    for _ in 0..5 {
        engine.publish("limited", "agent", payload.clone()).unwrap();
    }

    let events = engine.replay("limited", 0, Some(3)).unwrap();
    assert_eq!(events.len(), 3);
}

#[test]
fn subscribe_and_catch_up() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("coord", "Coordination").unwrap();

    let payload = EventPayload {
        status: AgentStatus::Working,
        working_on: "initial work".into(),
        waiting_for: None,
        can_start: None,
        verified: false,
        magellan_trace: None,
        extra: serde_json::Value::Null,
    };

    engine.publish("coord", "claude", payload.clone()).unwrap();
    engine.publish("coord", "claude", payload.clone()).unwrap();

    // Subscribe — should seed last_seen to current max (2)
    let sub = engine.subscribe("hermes", "coord").unwrap();
    assert_eq!(sub.last_seen_sequence, 2);
    assert!(!sub.created_at.is_empty(), "created_at should be set");
    assert!(!sub.updated_at.is_empty(), "updated_at should be set");
    assert_eq!(
        sub.created_at, sub.updated_at,
        "created_at and updated_at should match on fresh subscription"
    );

    // Publish more events after subscribing
    engine.publish("coord", "claude", payload.clone()).unwrap();
    engine.publish("coord", "claude", payload).unwrap();

    // Catch up should return events 3 and 4
    let new_events = engine.catch_up("hermes", "coord").unwrap();
    assert_eq!(new_events.len(), 2);
    assert_eq!(new_events[0].sequence_id, 3);
    assert_eq!(new_events[1].sequence_id, 4);

    // After catch_up, updated_at should be a valid RFC 3339 timestamp
    let updated_sub = engine.get_subscription("hermes", "coord").unwrap();
    assert!(!updated_sub.updated_at.is_empty());
    assert!(
        updated_sub.updated_at.contains('T'),
        "updated_at should be RFC 3339 after catch_up"
    );
}

#[test]
fn unsubscribe() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("temp", "").unwrap();

    engine.subscribe("agent", "temp").unwrap();
    let subs = engine.list_subscriptions("agent").unwrap();
    assert_eq!(subs.len(), 1);
    assert!(
        !subs[0].created_at.is_empty(),
        "subscription should have created_at"
    );
    assert!(
        !subs[0].updated_at.is_empty(),
        "subscription should have updated_at"
    );

    engine.unsubscribe("agent", "temp").unwrap();
    let subs = engine.list_subscriptions("agent").unwrap();
    assert!(subs.is_empty());
}

#[test]
fn unsubscribe_nonexistent_fails() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("ch", "").unwrap();
    let err = engine.unsubscribe("nobody", "ch").unwrap_err();
    assert!(err.to_string().contains("not subscribed"));
}

#[test]
fn status_reports_stats() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("c1", "").unwrap();
    engine.create_channel("c2", "").unwrap();

    let payload = EventPayload {
        status: AgentStatus::Done,
        working_on: "done".into(),
        waiting_for: None,
        can_start: None,
        verified: true,
        magellan_trace: None,
        extra: serde_json::Value::Null,
    };

    engine.publish("c1", "a", payload.clone()).unwrap();
    engine.publish("c1", "a", payload).unwrap();
    engine.subscribe("agent", "c1").unwrap();

    let stats = engine.status().unwrap();
    assert_eq!(stats.channels, 2);
    assert_eq!(stats.events, 2);
    assert_eq!(stats.subscriptions, 1);
}

#[test]
fn magellan_trace_roundtrips() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("traces", "").unwrap();

    let trace = envoy::MagellanTrace {
        files_changed: vec!["src/lib.rs".into()],
        symbols_added: vec!["fn new_func".into()],
        symbols_removed: vec![],
        db_state: Some(envoy::MagellanDbState {
            schema_version: 12,
            symbol_count: 583,
        }),
    };

    let payload = EventPayload {
        status: AgentStatus::Done,
        working_on: "added new_func".into(),
        waiting_for: None,
        can_start: Some("hermes can verify".into()),
        verified: true,
        magellan_trace: Some(trace),
        extra: serde_json::Value::Null,
    };

    let _event = engine.publish("traces", "claude", payload).unwrap();

    let events = engine.replay("traces", 0, None).unwrap();
    assert_eq!(events.len(), 1);

    let rt = events[0].payload.magellan_trace.as_ref().unwrap();
    assert_eq!(rt.files_changed, vec!["src/lib.rs"]);
    assert_eq!(rt.symbols_added, vec!["fn new_func"]);
    assert_eq!(rt.db_state.as_ref().unwrap().schema_version, 12);
    assert_eq!(rt.db_state.as_ref().unwrap().symbol_count, 583);
}

#[test]
fn error_display_messages() {
    let err = EnvoyError::AgentNotFound("id99".into());
    assert!(err.to_string().contains("id99"));

    let err = EnvoyError::AgentOffline("id1".into());
    assert!(err.to_string().contains("offline"));

    let err = EnvoyError::MessageNotFound("m-xxx".into());
    assert!(err.to_string().contains("m-xxx"));

    let err = EnvoyError::InvalidMessage("missing parts".into());
    assert!(err.to_string().contains("missing parts"));
}

// ── HTTP integration tests ──

use axum::body::Body;
use axum::http::StatusCode;
use axum::Router;
use std::sync::Arc;
use tower::ServiceExt; // for oneshot

fn test_app() -> Router {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_string_lossy().to_string();
    // Leak the TempDir so the DB survives for the test duration.
    // Integration tests are short-lived; disk cleanup happens at process exit.
    let _ = Box::leak(Box::new(dir));
    let engine = envoy::Engine::open(&db_path).unwrap();
    let state = Arc::new(envoy::http::AppState::new(engine).unwrap());
    envoy::http::build_router_unlimited(state)
}

#[tokio::test]
async fn register_agent_via_http() {
    let app = test_app();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"claude","kind":"claude"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["agent_id"], "id1");
    assert_eq!(json["name"], "claude");
}

#[tokio::test]
async fn list_agents() {
    let app = test_app();

    // Register two agents
    for (name, kind) in [("claude", "claude"), ("hermes", "hermes")] {
        app.clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/agents")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"name":"{}","kind":"{}"}}"#,
                        name, kind
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
    }

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/agents")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["agents"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn send_and_poll_messages() {
    let app = test_app();

    // Register two agents
    let register = |name: &str, kind: &str| {
        let body = format!(r#"{{"name":"{}","kind":"{}"}}"#, name, kind);
        async {
            app.clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/agents")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
        }
    };
    register("claude", "claude").await;
    register("hermes", "hermes").await;

    // Send a direct message from id1 to id2
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/messages")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"type":"direct","from":"id1","to":"id2","parts":[{"text":"hello hermes"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let msg: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(msg["sequence_id"], 1);

    // Poll messages for id2
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/messages?to=id2&since=0&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["messages"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn send_message_offline_agent_fails() {
    let app = test_app();

    // Register and immediately disconnect
    let register_body = r#"{"name":"ghost","kind":"claude"}"#;
    app.clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(register_body))
                .unwrap(),
        )
        .await
        .unwrap();

    app.clone()
        .oneshot(
            axum::http::Request::builder()
                .method("DELETE")
                .uri("/agents/id1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Register a second agent as recipient
    app.clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"hermes","kind":"hermes"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Try sending from the offline agent
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/messages")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"type":"direct","from":"id1","to":"id2","parts":[{"text":"hello?"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Explicitly deleted agent is gone (404), not just offline (409)
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn health_endpoint() {
    let app = test_app();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn websocket_connect_and_receive() {
    use futures::StreamExt;

    let app = test_app();

    // Register an agent
    app.clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"hermes","kind":"hermes"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Start the server on a random port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Small delay for server to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Connect WebSocket
    let url = format!("ws://{}/ws/id1", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

    // Should receive pending messages first (none), then agent_connected event
    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.to_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(json["event"], "agent_connected");
}

#[tokio::test]
async fn full_handoff_workflow() {
    use envoy::message::{
        CompletionStatus, HandoffData, MagellanTracePayload, PartContent, QualityGateResult,
        VerificationState, WhatIsStubbed, WhatWasDone,
    };
    use envoy::MessageType;

    let app = test_app();

    // 1. Register Claude (parent)
    let register_claude = |app: Router| async {
        app.oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"claude","kind":"claude"}"#))
                .unwrap(),
        )
        .await
        .unwrap()
    };
    register_claude(app.clone()).await;

    // 2. Register Claude subagent
    app.clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"name":"implement-task-3","kind":"claude","parent_id":"id1"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // 3. Subagent sends handoff to parent
    let handoff = HandoffData {
        completion_status: CompletionStatus::NeedsContext,
        blocked_reason: None,
        context_remaining_pct: 28,
        what_was_done: vec![WhatWasDone {
            scope: "src/engine.rs".into(),
            change: "added publish()".into(),
            verified: true,
        }],
        what_is_stubbed: vec![WhatIsStubbed {
            location: "src/http.rs".into(),
            reason: "context too low".into(),
        }],
        remaining_work: vec!["Implement HTTP server".into()],
        verification_state: VerificationState {
            tests_passing: 11,
            tests_failing: 0,
            quality_gate: QualityGateResult {
                passed: true,
                blocking: 0,
                warnings: 0,
            },
            cargo_check_passed: true,
        },
        magellan_trace: MagellanTracePayload {
            files_changed: vec!["src/engine.rs".into()],
            symbols_added: vec!["fn publish".into()],
            symbols_removed: vec![],
            refs_in: Default::default(),
            refs_out: Default::default(),
        },
        grounded_queries_used: vec!["magellan find --name Engine".into()],
    };

    let msg = envoy::http::SendMessageRequest {
        msg_type: MessageType::Handoff,
        from: "id1.1".into(),
        to: "id1".into(),
        task_id: Some("task-003".into()),
        context_id: Some("ctx-001".into()),
        parts: vec![
            envoy::message::Part {
                content: PartContent::Text("context at 28%, handing off".into()),
            },
            envoy::message::Part {
                content: PartContent::Data(serde_json::to_value(&handoff).unwrap()),
            },
        ],
    };

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/messages")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&msg).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .unwrap();
    let stored: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(stored["from"], "id1.1");
    assert_eq!(stored["to"], "id1");
    assert_eq!(stored["type"], "handoff");
    assert_eq!(stored["sequence_id"], 1);

    // 4. Parent polls and retrieves the handoff
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/messages?to=id1&since=0&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);

    // Verify handoff data round-trips through the Data part
    let handoff_part = &messages[0]["parts"][1];
    let roundtripped: HandoffData = serde_json::from_value(handoff_part["data"].clone()).unwrap();
    assert_eq!(
        roundtripped.completion_status,
        CompletionStatus::NeedsContext
    );
    assert_eq!(roundtripped.context_remaining_pct, 28);
    assert_eq!(roundtripped.remaining_work.len(), 1);
}

#[tokio::test]
async fn heartbeat_rejects_missing_working_on() {
    let app = test_app();

    // Register an agent first
    app.clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"test-agent","kind":"worker"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Send heartbeat without working_on field
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/heartbeat")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"agent_id":"id1","status":{"state":"working"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "heartbeat must reject missing working_on field"
    );
}

#[tokio::test]
async fn heartbeat_brings_agent_online_after_server_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_string_lossy().to_string();

    // Session 1: register agent
    let engine1 = envoy::Engine::open(&db_path).unwrap();
    let state1 = Arc::new(envoy::http::AppState::new(engine1).unwrap());
    let app1 = envoy::http::build_router_unlimited(state1);

    app1.clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"restart-test","kind":"worker"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Verify online after registration
    let resp = app1
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/agents")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let agents: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        agents["agents"][0]["online"], true,
        "fresh registration is online"
    );

    // Drop engine (simulates server shutdown)
    drop(app1);

    // Session 2: new engine from same DB (simulates restart)
    let engine2 = envoy::Engine::open(&db_path).unwrap();
    let state2 = Arc::new(envoy::http::AppState::new(engine2).unwrap());
    let app2 = envoy::http::build_router_unlimited(state2);

    // Verify agent starts offline after reload
    let resp = app2
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/agents")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let agents: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        agents["agents"][0]["online"], false,
        "agent must start offline after server restart"
    );

    // Send heartbeat
    let resp = app2
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/heartbeat")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"agent_id":"id1","status":{"state":"working","working_on":"reconnected"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify agent is now online
    let resp = app2
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/agents")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let agents: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        agents["agents"][0]["online"], true,
        "heartbeat must bring agent online after restart"
    );
}

#[tokio::test]
async fn message_ack_lifecycle() {
    let app = test_app();

    // Register two agents
    for (name, kind) in [("sender", "worker"), ("receiver", "worker")] {
        let body = format!(r#"{{"name":"{}","kind":"{}"}}"#, name, kind);
        app.clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/agents")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
    }

    // Send a message from id1 to id2
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/messages")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"type":"direct","from":"id1","to":"id2","parts":[{"text":"ack test"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let msg: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let msg_id = msg["message_id"].as_str().unwrap();

    // Poll unacked — message should appear
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/messages?to=id2&since=0&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["messages"].as_array().unwrap().len(),
        1,
        "unacked message should appear"
    );

    // ACK the message
    let ack_body = r#"{"agent_id":"id2"}"#.to_string();
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/messages/{}/ack", msg_id))
                .header("content-type", "application/json")
                .body(Body::from(ack_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let ack_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(ack_resp["acked_by"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("id2")));

    // Poll unacked — message should NOT appear
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/messages?to=id2&since=0&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["messages"].as_array().unwrap().len(),
        0,
        "acked message should not appear in unacked poll"
    );

    // Poll with include=acked — message should appear
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/messages?to=id2&since=0&limit=10&include=acked")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["messages"].as_array().unwrap().len(),
        1,
        "acked message should appear with include=acked"
    );
}

#[tokio::test]
async fn deliverable_verify_event_roundtrip() {
    let engine = Engine::open_in_memory().unwrap();
    let state = Arc::new(envoy::http::AppState::new(engine).unwrap());
    let app = envoy::http::build_router_unlimited(state.clone());

    // Ingest a verify event with all passed
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/events/verify")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "project": "envoy",
                        "agent_id": "id6",
                        "task_type": "feature",
                        "claimed_files": ["src/lib.rs"],
                        "passed": 1,
                        "failed": 0,
                        "failures": [],
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let event: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(event["event_type"], "task_verify");
    assert_eq!(event["severity"], "info");
    assert_eq!(event["data"]["passed"], 1);

    // Ingest a verify event with failures
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/events/verify")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "project": "envoy",
                        "agent_id": "id6",
                        "task_type": "bugfix",
                        "claimed_files": ["tests/missing.rs"],
                        "passed": 0,
                        "failed": 1,
                        "failures": ["tests/missing.rs: MISSING"],
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let event: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(event["event_type"], "task_verify");
    assert_eq!(event["severity"], "warning");
    assert_eq!(event["data"]["failed"], 1);
}

#[tokio::test]
async fn circuit_breaker_lifecycle() {
    // Register an agent
    let engine = Engine::open_in_memory().unwrap();
    let state = Arc::new(envoy::http::AppState::new(engine).unwrap());
    let app = envoy::http::build_router_unlimited(state.clone());

    // Register agent
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({"name": "test-agent", "kind": "worker"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let agent: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
            .unwrap();
    let agent_id = agent["agent_id"].as_str().unwrap();

    // Verify circuit breaker starts closed
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/agents/{agent_id}/circuit"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cb: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
            .unwrap();
    assert_eq!(cb["state"], "closed");
    assert_eq!(cb["failure_count"], 0);

    // Report delivery failures until threshold (default 5)
    for _ in 0..5 {
        let resp = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/agents/{agent_id}/circuit/failure"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Circuit should now be open
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/agents/{agent_id}/circuit"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cb: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
            .unwrap();
    assert_eq!(cb["state"], "open");

    // Heartbeat should implicitly close the circuit
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/heartbeat")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({
                        "agent_id": agent_id,
                        "status": {"state": "working", "working_on": "test"}
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Circuit should be closed again after heartbeat
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/agents/{agent_id}/circuit"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cb: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
            .unwrap();
    assert_eq!(cb["state"], "closed", "heartbeat should close circuit");
    assert_eq!(cb["failure_count"], 0, "failure count should reset");
}

#[tokio::test]
async fn offline_broadcast_stores_notification() {
    let app = test_app();

    // Register an agent (no WS connection = offline for push purposes)
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"offline-agent","kind":"claude"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let agent: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
            .unwrap();
    let agent_id = agent["agent_id"].as_str().unwrap();

    // Subscribe the agent to a project
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/subscriptions")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"agent_id":"{agent_id}","project":"test-proj"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Ingest a hook event — broadcast_to_project should store notification for offline agent
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/events/hook")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"project":"test-proj","hook_name":"verify-rust","exit_code":0,"output":"ok"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Poll messages — offline agent should see the stored notification
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/messages?to={agent_id}&since=0&limit=10"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let poll_body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 8192).await.unwrap())
            .unwrap();

    let messages = poll_body["messages"].as_array().unwrap();
    assert!(
        !messages.is_empty(),
        "offline agent should see stored notification"
    );

    let msg = &messages[0];
    assert_eq!(msg["from"], "envoy");
    assert_eq!(msg["type"], "system");
}

#[tokio::test]
async fn agent_registration_is_audited() {
    let app = test_app();

    // Register an agent
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"audit-agent","kind":"claude"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let agent: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
            .unwrap();
    let agent_id = agent["agent_id"].as_str().unwrap();

    // Query audit log for agent registration
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!(
                    "/audit?agent_id={agent_id}&operation=agent_registered"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let audit_body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 8192).await.unwrap())
            .unwrap();

    let events = audit_body["events"].as_array().unwrap();
    assert!(!events.is_empty(), "agent registration should be audited");
    assert_eq!(events[0]["source"], "agent_registered");
}

#[tokio::test]
async fn message_send_is_audited() {
    let app = test_app();

    // Register two agents
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"sender","kind":"claude"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let sender: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
            .unwrap();
    let sender_id = sender["agent_id"].as_str().unwrap();

    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"receiver","kind":"claude"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let receiver: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
            .unwrap();
    let receiver_id = receiver["agent_id"].as_str().unwrap();

    // Send a message
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/messages")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"type":"direct","from":"{sender_id}","to":"{receiver_id}","parts":[{{"text":"hello"}}]}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Query audit log for message sent
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!(
                    "/audit?agent_id={sender_id}&operation=message_sent"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let audit_body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 8192).await.unwrap())
            .unwrap();

    let events = audit_body["events"].as_array().unwrap();
    assert!(!events.is_empty(), "message send should be audited");
    assert_eq!(events[0]["source"], "message_sent");
}

#[tokio::test]
async fn event_ingestion_is_audited() {
    let app = test_app();

    // Register an agent
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/agents")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"event-agent","kind":"claude"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Ingest an event
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/events/hook")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"project":"test","hook_name":"verify","exit_code":0,"output":"ok"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Query audit log for event ingestion
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/audit?operation=event_ingested")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let audit_body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 8192).await.unwrap())
            .unwrap();

    let events = audit_body["events"].as_array().unwrap();
    assert!(!events.is_empty(), "event ingestion should be audited");
    assert_eq!(events[0]["source"], "event_ingested");
}

#[tokio::test]
async fn task_audit_trail_is_tagged() {
    let app = test_app();

    // Propose a task
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/tasks/propose")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"project":"test","description":"fix bug"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let task: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 4096).await.unwrap())
            .unwrap();
    let task_id = task["id"].as_str().unwrap();

    // Claim the task
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/tasks/{task_id}/claim"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"agent_id":"agent-1"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Query task audit trail
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/tasks/{task_id}/audit"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let audit_body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 8192).await.unwrap())
            .unwrap();

    let events = audit_body["events"].as_array().unwrap();
    assert!(!events.is_empty(), "task should have audit trail");
    assert_eq!(events[0]["source"], "task_claimed");
}
