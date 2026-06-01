//! HTTP routing, middleware, handlers, and WebSocket support for Envoy.
//!
//! Split from the monolithic `http.rs` (1968 LOC) into focused modules:
//! - `state` — shared application state (`AppState`, `SharedState`, `WsRegistry`)
//! - `middleware` — rate limiting and auth middleware
//! - `router` — route table construction (`build_router`, `build_router_unlimited`)
//! - `types` — request/response structs
//! - `handlers` — HTTP handler functions
//! - `ws` — WebSocket handler and broadcast logic

pub mod handlers;
pub mod middleware;
pub mod router;
pub mod state;
pub mod types;
pub mod ws;

// Re-export the public API surface so existing callers
// (`server.rs`, `lib.rs`, tests) continue to work unchanged.
pub use router::{build_router, build_router_unlimited};
pub use state::{run_nudge_loop, AppState, SharedState};
pub use types::{
    CreateDependencyRequest, HealthResponse, PollQuery, PollResponse, RegisterRequest,
    SendMessageRequest, StatsResponse,
};
