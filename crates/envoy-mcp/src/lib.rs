//! Envoy MCP Server
//!
//! A Model Context Protocol server exposing the envoy daemon's multi-agent
//! coordination API (agents, messaging, handoffs, discoveries, dependencies)
//! as MCP tools. All tool calls proxy to the running daemon over HTTP.

pub mod backend;
pub mod tools;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::ErrorData as McpError;

/// The MCP server instance.
pub struct EnvoyMcpServer {
    pub backend: std::sync::Arc<dyn backend::Backend>,
    pub router: ToolRouter<Self>,
}

impl EnvoyMcpServer {
    pub fn new(backend: std::sync::Arc<dyn backend::Backend>) -> Self {
        let mut router = ToolRouter::new();
        tools::register_all(&mut router);
        Self { backend, router }
    }
}

impl ServerHandler for EnvoyMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("envoy-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Envoy MCP server: tools for multi-agent coordination — agent \
                 registration, messaging, handoffs, discoveries, dependencies, \
                 knowledge graph search, and heartbeats.",
            )
            .with_protocol_version(rmcp::model::ProtocolVersion::V_2025_03_26)
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::ListToolsResult, McpError>>
           + rmcp::service::MaybeSendFuture
           + '_ {
        std::future::ready(Ok(rmcp::model::ListToolsResult {
            tools: self.router.list_all(),
            next_cursor: None,
            meta: None,
        }))
    }

    fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::CallToolResult, McpError>>
           + rmcp::service::MaybeSendFuture
           + '_ {
        let ctx = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        async move {
            self.router
                .call(ctx)
                .await
                .map_err(|e| McpError::internal_error(format!("tool call failed: {e}"), None))
        }
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        self.router.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ProtocolVersion;
    use serde_json::Value;
    use std::sync::Arc;

    struct MockBackend;

    #[async_trait::async_trait]
    impl backend::Backend for MockBackend {
        async fn health(&self) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"status": "ok"}))
        }
        async fn register_agent(
            &self,
            _n: &str,
            _k: &str,
            _p: Option<&str>,
        ) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"agent_id": "agent-1"}))
        }
        async fn list_agents(&self) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"agents": []}))
        }
        async fn send_message(
            &self,
            _t: &str,
            _f: &str,
            _to: &str,
            _p: &Value,
        ) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"message_id": "m-1"}))
        }
        async fn get_messages(&self, _to: &str, _l: i64) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"messages": []}))
        }
        async fn ack_message(&self, _id: &str) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"acknowledged": true}))
        }
        async fn store_discovery(
            &self,
            _a: &str,
            _t: &str,
            _ta: &str,
            _m: &Value,
        ) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"discovery_id": 42}))
        }
        async fn knowledge(&self, _t: &str) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"target": "test", "discoveries": []}))
        }
        async fn store_handoff(&self, _f: &str, _t: &str, _m: &Value) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"handoff_id": 7}))
        }
        async fn pending_handoff(&self, _a: &str, _p: Option<&str>) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"handoff": null}))
        }
        async fn claim_handoff(&self, _id: &str) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"claimed": true, "handoff_id": 7}))
        }
        async fn search(&self, _q: &str, _k: usize) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"results": [], "count": 0}))
        }
        async fn create_dependency(&self, _b: &str, _d: &str, _r: &str) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"dependency_id": 1}))
        }
        async fn graph_stats(&self) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"entity_count": 0, "edge_count": 0}))
        }
        async fn heartbeat(&self, _id: &str, _s: &Value) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"accepted": true, "nudges": []}))
        }

        async fn delegate_task(
            &self,
            _from: &str,
            _to: &str,
            goal: &str,
            _ctx: &str,
        ) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"handoff_id": "mock", "goal": goal}))
        }

        async fn declare_workflow(&self, edges: &Value) -> anyhow::Result<Value> {
            let count = edges.as_array().map(|a| a.len()).unwrap_or(0);
            Ok(serde_json::json!({"edges_declared": count}))
        }

        async fn status_snapshot(&self, agent: &str) -> anyhow::Result<Value> {
            Ok(serde_json::json!({"agent": agent, "agents_online": 1, "pending": 0}))
        }
    }

    fn mock_server() -> EnvoyMcpServer {
        EnvoyMcpServer::new(Arc::new(MockBackend))
    }

    #[test]
    fn server_info_is_correct() {
        let server = mock_server();
        let info = server.get_info();
        assert_eq!(info.protocol_version, ProtocolVersion::V_2025_03_26);
        assert_eq!(info.server_info.name, "envoy-mcp");
        assert!(info.instructions.is_some());
    }

    #[test]
    fn all_eighteen_tools_registered() {
        let server = mock_server();
        let tools = server.router.list_all();
        let names: Vec<_> = tools.iter().map(|t| t.name.as_ref()).collect();
        // Group 1: agent management
        assert!(names.contains(&"envoy_health"));
        assert!(names.contains(&"envoy_register_agent"));
        assert!(names.contains(&"envoy_list_agents"));
        // Group 2: messaging
        assert!(names.contains(&"envoy_send_message"));
        assert!(names.contains(&"envoy_get_messages"));
        assert!(names.contains(&"envoy_ack_message"));
        // Group 3: discoveries & handoffs
        assert!(names.contains(&"envoy_store_discovery"));
        assert!(names.contains(&"envoy_knowledge"));
        assert!(names.contains(&"envoy_store_handoff"));
        assert!(names.contains(&"envoy_pending_handoff"));
        assert!(names.contains(&"envoy_claim_handoff"));
        // Group 4: coordination
        assert!(names.contains(&"envoy_search"));
        assert!(names.contains(&"envoy_create_dependency"));
        assert!(names.contains(&"envoy_graph_stats"));
        assert!(names.contains(&"envoy_heartbeat"));
        assert_eq!(tools.len(), 18);
    }

    #[test]
    fn get_tool_by_name() {
        let server = mock_server();
        assert!(server.get_tool("envoy_health").is_some());
        assert!(server.get_tool("envoy_search").is_some());
        assert!(server.get_tool("nonexistent").is_none());
    }
}
