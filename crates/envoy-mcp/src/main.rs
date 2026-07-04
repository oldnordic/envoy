use std::sync::Arc;

use envoy_mcp::backend::Backend;
use envoy_mcp::{backend, EnvoyMcpServer};
use rmcp::transport::stdio;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let base_url =
        std::env::var("ENVOY_URL").unwrap_or_else(|_| "http://localhost:9876".to_string());

    let backend = backend::HttpBackend::new(&base_url);

    // Auto-register with the daemon so we get an agent_id for authenticated calls.
    // The calling tool identifies itself via ENVOY_AGENT_NAME (e.g. "claude-code",
    // "codex", "kimi-code", "hermes-agent"). If unset, default to "mcp-client".
    let agent_name = std::env::var("ENVOY_AGENT_NAME").unwrap_or_else(|_| "mcp-client".to_string());
    tracing::info!("envoy-mcp starting (daemon: {base_url}, agent_name: {agent_name})");

    match backend.register_agent(&agent_name, "mcp", None).await {
        Ok(reg) => {
            tracing::info!("Auto-registered as '{agent_name}' with daemon");
            if let Some(id) = reg.get("agent_id").and_then(|v| v.as_str()) {
                tracing::info!("Agent ID: {id}");
            }
        }
        Err(e) => {
            tracing::warn!("Auto-registration failed (daemon may be offline): {e}");
            tracing::warn!("Tools requiring auth will return 401 until the daemon is reachable.");
        }
    }

    let backend: Arc<dyn backend::Backend> = Arc::new(backend);
    let server = EnvoyMcpServer::new(backend);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
