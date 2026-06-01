use axum::{routing::get, Router};

use crate::http::handlers::*;
use crate::http::middleware::rate_limit_middleware;
use crate::http::state::SharedState;
use crate::http::ws::ws_handler;

fn build_base_routes() -> Router<SharedState> {
    Router::new()
        .route("/agents", get(list_agents).post(register_agent))
        .route(
            "/agents/{agent_id}",
            get(get_agent).delete(disconnect_agent),
        )
        .route(
            "/agents/{agent_id}/retire",
            axum::routing::post(retire_agent),
        )
        .route("/agents/{agent_id}/messages/pending", get(pending_messages))
        .route("/messages", get(poll_messages).post(send_message))
        .route("/messages/{message_id}", get(get_message))
        .route(
            "/messages/{message_id}/ack",
            axum::routing::post(ack_message),
        )
        .route("/agents/{agent_id}/circuit", get(get_circuit))
        .route(
            "/agents/{agent_id}/circuit/failure",
            axum::routing::post(record_circuit_failure),
        )
        .route("/health", get(health))
        .route("/stats", get(stats))
        .route("/heartbeat", axum::routing::post(heartbeat))
        .route("/dependencies", axum::routing::post(create_dependency))
        .route("/dependencies/blocker/{agent_id}", get(get_blocker_deps))
        .route(
            "/dependencies/dependent/{agent_id}",
            get(get_dependent_deps),
        )
        .route(
            "/dependencies/{dep_id}/resolve",
            axum::routing::post(resolve_dependency),
        )
        .route(
            "/nudge-config",
            get(get_nudge_config).post(update_nudge_config),
        )
        .route("/events/hook", axum::routing::post(ingest_hook_event))
        .route("/events/gate", axum::routing::post(ingest_gate_event))
        .route("/events/ci", axum::routing::post(ingest_ci_event))
        .route("/events/doc", axum::routing::post(ingest_doc_event))
        .route("/events/verify", axum::routing::post(ingest_verify_event))
        .route("/events", get(query_events))
        .route("/audit", get(query_audit))
        .route("/tasks/propose", axum::routing::post(propose_task))
        .route("/tasks/claim-next", axum::routing::post(claim_next_task))
        .route("/tasks/{id}/claim", axum::routing::post(claim_task))
        .route("/tasks/{id}/state", axum::routing::post(update_task_state))
        .route("/tasks/{id}/audit", get(query_task_audit))
        .route("/tasks/{id}", get(get_task))
        .route("/tasks", get(list_tasks))
        .route("/subscriptions", axum::routing::post(subscribe_agent))
        .route(
            "/subscriptions/{agent_id}/{project}",
            axum::routing::delete(unsubscribe_agent),
        )
        .route("/subscriptions/{agent_id}", get(list_subscriptions))
        .route(
            "/projects/{name}/config",
            get(get_project_config).post(set_project_config),
        )
        .route("/ws/{agent_id}", get(ws_handler))
}

#[cfg(feature = "atheneum")]
fn add_atheneum_routes(routes: Router<SharedState>) -> Router<SharedState> {
    crate::atheneum_bridge::add_atheneum_routes(routes)
}

/// Build the envoy HTTP router with rate limiting.
pub fn build_router(state: SharedState) -> Router {
    let routes = build_base_routes();

    #[cfg(feature = "atheneum")]
    let routes = add_atheneum_routes(routes);

    routes
        .with_state(state.clone())
        .layer(axum::extract::DefaultBodyLimit::max(1_048_576))
        .layer(axum::middleware::from_fn_with_state(
            state,
            rate_limit_middleware,
        ))
}

/// Build the router without rate limiting (for tests).
pub fn build_router_unlimited(state: SharedState) -> Router {
    let routes = build_base_routes();

    #[cfg(feature = "atheneum")]
    let routes = add_atheneum_routes(routes);

    routes.with_state(state)
}
