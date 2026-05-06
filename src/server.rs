use std::net::SocketAddr;
use std::sync::Arc;

use crate::engine::Engine;
use crate::error::Result;
use crate::http::{build_router, run_nudge_loop, AppState};

/// Run the envoy server. Opens (or creates) the database at `db_path`
/// and starts the HTTP server on `addr`.
pub async fn run(db_path: &str, addr: SocketAddr) -> Result<()> {
    let engine = Engine::open(db_path)?;
    let state = Arc::new(AppState::new(engine)?);

    // Spawn background nudge loop
    let nudge_state = state.clone();
    tokio::spawn(async move {
        run_nudge_loop(nudge_state).await;
    });

    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| crate::error::EnvoyError::WsError(format!("failed to bind {addr}: {e}")))?;

    println!("envoy server listening on {addr}, db={db_path}");
    axum::serve(listener, app)
        .await
        .map_err(|e| crate::error::EnvoyError::WsError(format!("server error: {e}")))?;

    Ok(())
}
