use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use crate::error::Result;
use crate::http::{build_router, AppState};

/// Run the envoy server. Opens (or creates) the database at `db_path`
/// and starts the HTTP server on `addr`.
pub async fn run(db_path: &str, addr: SocketAddr) -> Result<()> {
    let conn = Arc::new(Mutex::new(rusqlite::Connection::open(db_path)?));
    let state = Arc::new(AppState::new(conn));

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
