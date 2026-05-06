use std::net::SocketAddr;
use std::sync::Arc;

use crate::engine::Engine;
use crate::error::Result;
use crate::http::{build_router, run_nudge_loop, AppState};
use crate::monitor::{ci, doc};

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

    // Background event purge (every hour)
    let purge_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            if let Ok(engine) = purge_state.engine.lock() {
                if let Ok(purged) = purge_state.event_bus.purge_old_events(engine.graph()) {
                    if purged > 0 {
                        eprintln!("purged {} events older than 24h", purged);
                    }
                }
            }
        }
    });

    // Spawn CI monitor for magellan
    let ci_state = state.clone();
    tokio::spawn(async move {
        ci::run_ci_monitor(ci_state, "magellan".into(), "oldnordic/magellan".into(), 60).await;
    });

    // Spawn doc monitor for magellan
    let doc_state = state.clone();
    tokio::spawn(async move {
        doc::run_doc_monitor(doc_state, "magellan".into(), ".".into(), 300).await;
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
