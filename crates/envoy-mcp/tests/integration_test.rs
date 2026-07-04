use std::sync::Arc;

use envoy_mcp::{backend, EnvoyMcpServer};
use rmcp::ServiceExt;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct MockBackend;

#[async_trait::async_trait]
impl backend::Backend for MockBackend {
    async fn health(&self) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"status": "ok", "uptime_seconds": 100, "agents_online": 1}))
    }
    async fn register_agent(
        &self,
        _n: &str,
        _k: &str,
        p: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"agent_id": "agent-1", "name": "tester", "kind": "claude", "parent_id": p}))
    }
    async fn list_agents(&self) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"agents": [{"agent_id": "agent-1", "name": "tester"}]}))
    }
    async fn send_message(
        &self,
        _t: &str,
        _f: &str,
        _to: &str,
        _p: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"message_id": "m-1", "accepted": true}))
    }
    async fn get_messages(&self, _to: &str, _l: i64) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"messages": [], "latest_sequence": 0}))
    }
    async fn ack_message(&self, _id: &str) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"acknowledged": true}))
    }
    async fn store_discovery(
        &self,
        _a: &str,
        _t: &str,
        _ta: &str,
        _m: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"discovery_id": 42}))
    }
    async fn knowledge(&self, _t: &str) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"target": "test", "discovery_count": 0}))
    }
    async fn store_handoff(
        &self,
        _f: &str,
        _t: &str,
        _m: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"handoff_id": 7}))
    }
    async fn pending_handoff(
        &self,
        _a: &str,
        _p: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"handoff": null}))
    }
    async fn claim_handoff(&self, _id: &str) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"claimed": true, "handoff_id": 7}))
    }
    async fn search(&self, _q: &str, _k: usize) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"query": "test", "count": 0, "results": []}))
    }
    async fn create_dependency(
        &self,
        _b: &str,
        _d: &str,
        _r: &str,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"dependency_id": 1}))
    }
    async fn graph_stats(&self) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"entity_count": 5, "edge_count": 8}))
    }
    async fn heartbeat(
        &self,
        _id: &str,
        _s: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"accepted": true, "nudges": []}))
    }

    async fn delegate_task(
        &self,
        _from: &str,
        _to: &str,
        goal: &str,
        _ctx: &str,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"handoff_id": "mock", "goal": goal}))
    }

    async fn declare_workflow(
        &self,
        edges: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let count = edges.as_array().map(|a| a.len()).unwrap_or(0);
        Ok(json!({"edges_declared": count}))
    }

    async fn status_snapshot(&self, agent: &str) -> anyhow::Result<serde_json::Value> {
        Ok(json!({"agent": agent, "agents_online": 1, "pending": 0}))
    }
}

async fn send_json(w: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>, msg: serde_json::Value) {
    let line = msg.to_string() + "\n";
    w.write_all(line.as_bytes()).await.unwrap();
    w.flush().await.unwrap();
}

async fn recv_json(
    r: &mut BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
) -> serde_json::Value {
    let mut line = String::new();
    r.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn mcp_server_initializes_and_lists_tools() {
    let (server_stream, client_stream) = tokio::io::duplex(4096);
    let (client_read, client_write) = tokio::io::split(client_stream);
    let mut client_reader = BufReader::new(client_read);
    let mut client_writer = client_write;

    let server = EnvoyMcpServer::new(Arc::new(MockBackend));
    let server_handle = tokio::spawn(async move {
        let running = server.serve(server_stream).await.unwrap();
        running.waiting().await.unwrap();
    });

    // Initialize
    send_json(
        &mut client_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "test-client", "version": "0.0.1" }
            }
        }),
    )
    .await;

    let init_response = recv_json(&mut client_reader).await;
    assert_eq!(init_response["id"], 1);
    assert!(init_response["result"]["serverInfo"]["name"]
        .as_str()
        .unwrap()
        .contains("envoy-mcp"));

    // Initialized notification
    send_json(
        &mut client_writer,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;

    // List tools
    send_json(
        &mut client_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    )
    .await;

    let list_response = recv_json(&mut client_reader).await;
    assert_eq!(list_response["id"], 2);
    let tools = list_response["result"]["tools"].as_array().unwrap();
    let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(tools.len(), 18, "expected 18 tools, got {}", tools.len());
    assert!(names.contains(&"envoy_health"));
    assert!(names.contains(&"envoy_send_message"));
    assert!(names.contains(&"envoy_graph_stats"));
    assert!(names.contains(&"envoy_heartbeat"));

    // Call envoy_health tool
    send_json(
        &mut client_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "envoy_health",
                "arguments": {}
            }
        }),
    )
    .await;

    let tool_response = recv_json(&mut client_reader).await;
    assert_eq!(tool_response["id"], 3);
    let content = tool_response["result"]["content"].as_array().unwrap();
    assert!(!content.is_empty());
    let text = content[0]["text"].as_str().unwrap();
    assert!(text.contains("status"));

    // Clean shutdown
    drop(client_writer);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server_handle).await;
}

#[tokio::test]
async fn mcp_tool_call_with_args() {
    let (server_stream, client_stream) = tokio::io::duplex(4096);
    let (client_read, client_write) = tokio::io::split(client_stream);
    let mut client_reader = BufReader::new(client_read);
    let mut client_writer = client_write;

    let server = EnvoyMcpServer::new(Arc::new(MockBackend));
    let server_handle = tokio::spawn(async move {
        let running = server.serve(server_stream).await.unwrap();
        running.waiting().await.unwrap();
    });

    // Initialize
    send_json(
        &mut client_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "test-client", "version": "0.0.1" }
            }
        }),
    )
    .await;
    let _ = recv_json(&mut client_reader).await;

    send_json(
        &mut client_writer,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;

    // Call envoy_search with args
    send_json(
        &mut client_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "envoy_search",
                "arguments": {
                    "q": "memory leak",
                    "k": 5
                }
            }
        }),
    )
    .await;

    let tool_response = recv_json(&mut client_reader).await;
    assert_eq!(tool_response["id"], 2);
    let content = tool_response["result"]["content"].as_array().unwrap();
    let text = content[0]["text"].as_str().unwrap();
    assert!(text.contains("results"));

    // Clean shutdown
    drop(client_writer);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server_handle).await;
}

#[tokio::test]
async fn mcp_register_and_handoff_optional_args_are_accepted() {
    let (server_stream, client_stream) = tokio::io::duplex(4096);
    let (client_read, client_write) = tokio::io::split(client_stream);
    let mut client_reader = BufReader::new(client_read);
    let mut client_writer = client_write;

    let server = EnvoyMcpServer::new(Arc::new(MockBackend));
    let server_handle = tokio::spawn(async move {
        let running = server.serve(server_stream).await.unwrap();
        running.waiting().await.unwrap();
    });

    send_json(
        &mut client_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "test-client", "version": "0.0.1" }
            }
        }),
    )
    .await;
    let _ = recv_json(&mut client_reader).await;
    send_json(
        &mut client_writer,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;

    send_json(
        &mut client_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "envoy_register_agent",
                "arguments": {
                    "name": "child-agent",
                    "kind": "codex",
                    "parent_id": "id1"
                }
            }
        }),
    )
    .await;
    let register_response = recv_json(&mut client_reader).await;
    let register_text = register_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(register_text.contains("\"parent_id\": \"id1\""));

    send_json(
        &mut client_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "envoy_pending_handoff",
                "arguments": {
                    "agent": "id1.1",
                    "project": "sqlitegraph"
                }
            }
        }),
    )
    .await;
    let handoff_response = recv_json(&mut client_reader).await;
    let handoff_text = handoff_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(handoff_text.contains("\"handoff\": null"));

    drop(client_writer);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server_handle).await;
}
