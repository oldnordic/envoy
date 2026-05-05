use envoy::{AgentStatus, Engine, EnvoyError, EventPayload};

#[test]
fn create_and_get_channel() {
    let engine = Engine::open_in_memory().unwrap();
    let channel = engine
        .create_channel("test-channel", "A test channel")
        .unwrap();
    assert_eq!(channel.name, "test-channel");
    assert_eq!(channel.description, "A test channel");

    let found = engine.get_channel("test-channel").unwrap();
    assert_eq!(found.id, channel.id);

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

    // Publish more events after subscribing
    engine.publish("coord", "claude", payload.clone()).unwrap();
    engine.publish("coord", "claude", payload).unwrap();

    // Catch up should return events 3 and 4
    let new_events = engine.catch_up("hermes", "coord").unwrap();
    assert_eq!(new_events.len(), 2);
    assert_eq!(new_events[0].sequence_id, 3);
    assert_eq!(new_events[1].sequence_id, 4);
}

#[test]
fn unsubscribe() {
    let engine = Engine::open_in_memory().unwrap();
    engine.create_channel("temp", "").unwrap();

    engine.subscribe("agent", "temp").unwrap();
    let subs = engine.list_subscriptions("agent").unwrap();
    assert_eq!(subs.len(), 1);

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
use std::sync::{Arc, Mutex};
use tower::ServiceExt; // for oneshot

fn test_app() -> Router {
    let conn = Arc::new(Mutex::new(rusqlite::Connection::open_in_memory().unwrap()));
    let state = Arc::new(envoy::http::AppState::new(conn));
    envoy::http::build_router(state)
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

    assert_eq!(response.status(), StatusCode::CONFLICT);
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
