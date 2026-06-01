use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::http::state::{recover_lock, SharedState};

pub async fn rate_limit_middleware(
    State(state): State<SharedState>,
    request: Request,
    next: Next,
) -> Response {
    // Public endpoints: health checks and agent registration bypass auth.
    let path = request.uri().path();
    let is_health = path == "/health" || path == "/stats";
    let is_registration = path == "/agents" && request.method() == "POST";

    if is_health || is_registration {
        return next.run(request).await;
    }

    let agent_id = request
        .headers()
        .get("x-agent-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if agent_id.is_empty() || !state.agent_registry.is_active(agent_id).unwrap_or(false) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let decision = {
        let engine = recover_lock(&state.engine);
        state
            .rate_limiter
            .check_rate_limit(engine.graph(), agent_id)
    };

    if !decision.allowed {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }

    next.run(request).await
}
