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

    // Background event + delivery purge (every hour)
    let purge_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            let state_fb = purge_state.clone();
            let (events_purged, deliveries_purged) = tokio::task::spawn_blocking(move || {
                let engine = state_fb.engine.lock().unwrap();
                let ep = state_fb
                    .event_bus
                    .purge_old_events(engine.graph())
                    .unwrap_or(0);
                let dp = state_fb
                    .delivery_tracker
                    .purge_deliveries(engine.graph())
                    .unwrap_or(0);
                (ep, dp)
            })
            .await
            .unwrap_or((0, 0));
            if events_purged > 0 {
                eprintln!("purged {} events older than 24h", events_purged);
            }
            if deliveries_purged > 0 {
                eprintln!(
                    "purged {} delivery records older than 24h",
                    deliveries_purged
                );
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
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| crate::error::EnvoyError::WsError(format!("server error: {e}")))?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C handler");
    eprintln!("received shutdown signal, draining...");
}
