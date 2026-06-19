use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use crate::engine::Engine;
use crate::error::Result;
use crate::http::{build_router, run_nudge_loop, AppState};
use crate::monitor::{ci, doc};

/// The default local-RPC socket path: `$XDG_RUNTIME_DIR/envoy.sock`
/// (typically `/run/user/<uid>/envoy.sock`), falling back to
/// `/tmp/envoy-<uid>.sock` when no runtime dir is available.
pub fn default_socket_path() -> std::path::PathBuf {
    if let Ok(rt) = std::env::var("XDG_RUNTIME_DIR") {
        return std::path::PathBuf::from(rt).join("envoy.sock");
    }
    let uid = std::process::id();
    std::path::PathBuf::from(format!("/tmp/envoy-{uid}.sock"))
}

/// Open the engine, build shared state, and spawn all background tasks
/// (nudge loop, hourly purge, optional CI/doc monitors). Returns the state
/// ready to be served over any transport. This is the single shared daemon
/// core — both `run_with_atheneum` (HTTP) and `run_local` (Unix socket) use
/// it so they run identical business logic.
fn setup_state(
    db_path: &str,
    #[allow(unused_variables)] atheneum_path: Option<String>,
) -> Result<Arc<AppState>> {
    // Initialize Prometheus metrics recorder (idempotent via LazyLock).
    crate::metrics::init();

    let engine = Engine::open(db_path)?;
    #[cfg(feature = "atheneum")]
    let state = Arc::new(AppState::new(engine)?.with_atheneum(atheneum_path));
    #[cfg(not(feature = "atheneum"))]
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
            let (events_purged, deliveries_purged, agents_purged, circuits_evicted) =
                tokio::task::spawn_blocking(move || {
                    let engine = state_fb.engine.lock();
                    let ep = state_fb
                        .event_bus
                        .purge_old_events(engine.graph())
                        .unwrap_or(0);
                    let dp = state_fb
                        .delivery_tracker
                        .purge_deliveries(engine.graph())
                        .unwrap_or(0);
                    let ap = state_fb.agent_registry.purge_retired(24).unwrap_or(0);
                    let ce = state_fb.circuit_breaker.evict_stale();
                    (ep, dp, ap, ce)
                })
                .await
                .unwrap_or((0, 0, 0, 0));
            if events_purged > 0 {
                eprintln!("purged {} events older than 24h", events_purged);
            }
            if deliveries_purged > 0 {
                eprintln!(
                    "purged {} delivery records older than 24h",
                    deliveries_purged
                );
            }
            if agents_purged > 0 {
                eprintln!("purged {} agents offline >24h", agents_purged);
            }
            if circuits_evicted > 0 {
                eprintln!("evicted {} stale circuit breaker entries", circuits_evicted);
            }
        }
    });

    if let Ok(ci_cfg) = std::env::var("ENVOY_CI_MONITOR") {
        let parts: Vec<&str> = ci_cfg.splitn(3, ',').collect();
        if parts.len() == 3 {
            let project = parts[0].to_string();
            let owner_repo = parts[1].to_string();
            let interval: u64 = parts[2].parse().unwrap_or(60);
            let ci_state = state.clone();
            tokio::spawn(async move {
                ci::run_ci_monitor(ci_state, project, owner_repo, interval).await;
            });
        } else {
            eprintln!("ENVOY_CI_MONITOR format: project,owner/repo,interval_secs");
        }
    }

    if let Ok(doc_cfg) = std::env::var("ENVOY_DOC_MONITOR") {
        let parts: Vec<&str> = doc_cfg.splitn(3, ',').collect();
        if parts.len() == 3 {
            let project = parts[0].to_string();
            let repo_path = parts[1].to_string();
            let interval: u64 = parts[2].parse().unwrap_or(300);
            let doc_state = state.clone();
            tokio::spawn(async move {
                doc::run_doc_monitor(doc_state, project, repo_path, interval).await;
            });
        } else {
            eprintln!("ENVOY_DOC_MONITOR format: project,repo_path,interval_secs");
        }
    }

    Ok(state)
}

/// Run the envoy HTTP server. Opens (or creates) the database at `db_path`
/// and starts serving HTTP on `addr`. This is the network + universal curl
/// transport — opt-in via `envoy serve`.
pub async fn run(db_path: &str, addr: SocketAddr) -> Result<()> {
    run_with_atheneum(db_path, addr, None).await
}

/// Run the envoy HTTP server with atheneum integration enabled.
pub async fn run_with_atheneum(
    db_path: &str,
    addr: SocketAddr,
    atheneum_path: Option<String>,
) -> Result<()> {
    let state = setup_state(db_path, atheneum_path)?;
    let app = build_router(state).into_make_service_with_connect_info::<std::net::SocketAddr>();
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| crate::error::EnvoyError::WsError(format!("failed to bind {addr}: {e}")))?;

    println!("envoy serving HTTP on {addr}, db={db_path}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| crate::error::EnvoyError::WsError(format!("server error: {e}")))?;

    Ok(())
}

/// Run the envoy local-RPC server over a Unix domain socket. Opens (or
/// creates) the database at `db_path` and serves the identical router over
/// `socket_path` — same business logic as HTTP, no TCP port. This is the
/// local-dev primary transport, opt-in via `envoy local`.
///
/// A stale socket file from a previous (crashed) run is removed before
/// binding. The socket is created with mode 0600 (owner-only). On graceful
/// shutdown the socket file is cleaned up.
pub async fn run_local(
    db_path: &str,
    socket_path: impl AsRef<Path>,
    atheneum_path: Option<String>,
) -> Result<()> {
    let socket_path = socket_path.as_ref();
    let state = setup_state(db_path, atheneum_path)?;
    let app = build_router(state).into_make_service();

    // Remove a stale socket from a previous crashed run.
    let _ = std::fs::remove_file(socket_path);
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            crate::error::EnvoyError::WsError(format!(
                "failed to create socket dir {}: {e}",
                parent.display()
            ))
        })?;
    }

    let listener = tokio::net::UnixListener::bind(socket_path).map_err(|e| {
        crate::error::EnvoyError::WsError(format!(
            "failed to bind socket {}: {e}",
            socket_path.display()
        ))
    })?;

    // Restrict to owner-only (0600).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600));
    }

    println!(
        "envoy serving local RPC on {}, db={db_path}",
        socket_path.display()
    );
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    // Always clean up the socket file on exit.
    let _ = std::fs::remove_file(socket_path);

    result.map_err(|e| crate::error::EnvoyError::WsError(format!("server error: {e}")))?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C handler");
    eprintln!("received shutdown signal, draining...");
}
