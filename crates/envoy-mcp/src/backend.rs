//! Backend abstraction for Envoy operations.
//!
//! The MCP server is decoupled from the running envoy daemon via the
//! [`Backend`] trait. The only implementation provided is [`HttpBackend`],
//! which talks to the daemon's HTTP API (default `http://localhost:9876`).
//! Every method issues a real HTTP request using `reqwest`.

use anyhow::Result;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

// -----------------------------------------------------------------------
// Backend trait — one async method per MCP tool.
// -----------------------------------------------------------------------

#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// GET /health — daemon liveness probe.
    async fn health(&self) -> Result<Value>;
    /// POST /agents — register a new agent.
    async fn register_agent(
        &self,
        name: &str,
        kind: &str,
        parent_id: Option<&str>,
    ) -> Result<Value>;
    /// GET /agents — list all registered agents.
    async fn list_agents(&self) -> Result<Value>;
    /// POST /messages — send a message to another agent.
    async fn send_message(
        &self,
        msg_type: &str,
        from: &str,
        to: &str,
        parts: &Value,
    ) -> Result<Value>;
    /// GET /messages?to=&limit= — poll messages addressed to an agent.
    async fn get_messages(&self, to: &str, limit: i64) -> Result<Value>;
    /// POST /messages/{id}/ack — acknowledge a message.
    async fn ack_message(&self, id: &str) -> Result<Value>;
    /// POST /atheneum/discoveries — store a finding in the knowledge graph.
    async fn store_discovery(
        &self,
        agent: &str,
        discovery_type: &str,
        target: &str,
        metadata: &Value,
    ) -> Result<Value>;
    /// GET /atheneum/knowledge?target= — aggregated knowledge for a target.
    async fn knowledge(&self, target: &str) -> Result<Value>;
    /// POST /atheneum/handoffs — create a pending task handoff.
    async fn store_handoff(
        &self,
        from_agent: &str,
        to_agent: &str,
        manifest: &Value,
    ) -> Result<Value>;
    /// GET /atheneum/handoffs/pending?agent= — pending handoffs for an agent.
    async fn pending_handoff(&self, agent: &str, project: Option<&str>) -> Result<Value>;
    /// POST /atheneum/handoffs/{id}/claim — claim a handoff.
    async fn claim_handoff(&self, id: &str) -> Result<Value>;
    /// GET /atheneum/search?q=&k= — lexical search over the knowledge graph.
    async fn search(&self, q: &str, k: usize) -> Result<Value>;
    /// POST /dependencies — declare a task dependency.
    async fn create_dependency(
        &self,
        blocker: &str,
        dependent: &str,
        reason: &str,
    ) -> Result<Value>;
    /// GET /atheneum/graph/stats — high-level graph topology stats.
    async fn graph_stats(&self) -> Result<Value>;
    /// POST /heartbeat — send a heartbeat for an agent.
    async fn heartbeat(&self, agent_id: &str, status: &Value) -> Result<Value>;

    // --- Composite tools ---

    /// Delegate a task: create a handoff and return immediately. The receiving
    /// agent picks it up, works, and stores the result back.
    async fn delegate_task(
        &self,
        from_agent: &str,
        to_agent: &str,
        goal: &str,
        context: &str,
    ) -> Result<Value>;

    /// Declare a multi-agent workflow as a dependency graph. Each edge says
    /// "blocker must finish before dependent starts." Envoy enforces ordering.
    async fn declare_workflow(&self, edges: &Value) -> Result<Value>;

    /// Dashboard view: which agents are online, what handoffs are pending,
    /// what dependencies are unresolved. One call replaces 3 round trips.
    async fn status_snapshot(&self, agent: &str) -> Result<Value>;
}

// -----------------------------------------------------------------------
// HTTP backend
// -----------------------------------------------------------------------

/// HTTP client backend that proxies every call to the running envoy daemon.
///
/// On startup, the backend auto-registers with the daemon (using a name
/// derived from `ENVOY_AGENT_NAME` env or "mcp-client") and captures the
/// server-assigned `agent_id`. This ID is injected as `X-Agent-Id` on all
/// subsequent requests for authentication. Each MCP-compatible tool
/// (Claude Code, Codex, Hermes, etc.) sets `ENVOY_AGENT_NAME` to its own
/// identity so the daemon can distinguish them.
pub struct HttpBackend {
    client: reqwest::Client,
    base_url: String,
    agent_id: parking_lot::Mutex<Option<String>>,
}

impl HttpBackend {
    /// Create a new backend pointed at `base_url` (no trailing slash).
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into();
        let trimmed = base_url.trim_end_matches('/').to_string();
        Self {
            client: reqwest::Client::new(),
            base_url: trimmed,
            agent_id: parking_lot::Mutex::new(None),
        }
    }

    /// Set the agent ID (called after successful registration or from env).
    pub fn set_agent_id(&self, id: impl Into<String>) {
        *self.agent_id.lock() = Some(id.into());
    }

    /// Build a request builder with the `X-Agent-Id` header attached (if set).
    fn with_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let guard = self.agent_id.lock();
        if let Some(ref id) = *guard {
            builder.header("X-Agent-Id", id)
        } else {
            builder
        }
    }

    async fn post_json<P: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        payload: &P,
    ) -> Result<R> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .with_auth(self.client.post(&url).json(payload))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("HTTP {status} error from {url}: {text}"));
        }
        Ok(resp.json().await?)
    }

    async fn get_json<R: DeserializeOwned>(&self, path: &str) -> Result<R> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.with_auth(self.client.get(&url)).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("HTTP {status} error from {url}: {text}"));
        }
        Ok(resp.json().await?)
    }

    /// POST with no request body (used by ack/claim endpoints).
    async fn post_empty<R: DeserializeOwned>(&self, path: &str) -> Result<R> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .with_auth(self.client.post(&url))
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("HTTP {status} error from {url}: {text}"));
        }
        Ok(resp.json().await?)
    }
}

#[async_trait]
impl Backend for HttpBackend {
    async fn health(&self) -> Result<Value> {
        self.get_json("/health").await
    }

    async fn register_agent(
        &self,
        name: &str,
        kind: &str,
        parent_id: Option<&str>,
    ) -> Result<Value> {
        let payload = serde_json::json!({
            "name": name,
            "kind": kind,
            "parent_id": parent_id,
        });
        // Registration is a public endpoint — no auth header needed.
        let url = format!("{}/agents", self.base_url);
        let resp = self.client.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("HTTP {status} error from {url}: {text}"));
        }
        let result: Value = resp.json().await?;
        // Auto-capture the server-assigned agent_id for future authenticated calls.
        if let Some(id) = result.get("agent_id").and_then(|v| v.as_str()) {
            tracing::info!("Registered agent '{name}' → agent_id={id}");
            self.set_agent_id(id);
        }
        Ok(result)
    }

    async fn list_agents(&self) -> Result<Value> {
        self.get_json("/agents").await
    }

    async fn send_message(
        &self,
        msg_type: &str,
        from: &str,
        to: &str,
        parts: &Value,
    ) -> Result<Value> {
        let payload = serde_json::json!({
            "type": msg_type,
            "from": from,
            "to": to,
            "parts": parts,
        });
        self.post_json("/messages", &payload).await
    }

    async fn get_messages(&self, to: &str, limit: i64) -> Result<Value> {
        let path = format!("/messages?to={}&limit={}", encode(to), limit);
        self.get_json(&path).await
    }

    async fn ack_message(&self, id: &str) -> Result<Value> {
        let path = format!("/messages/{}/ack", encode(id));
        let agent_id = self
            .agent_id
            .lock()
            .clone()
            .unwrap_or_default();
        self.post_json(&path, &serde_json::json!({"agent_id": agent_id}))
            .await
    }

    async fn store_discovery(
        &self,
        agent: &str,
        discovery_type: &str,
        target: &str,
        metadata: &Value,
    ) -> Result<Value> {
        let payload = serde_json::json!({
            "agent": agent,
            "discovery_type": discovery_type,
            "target": target,
            "metadata": metadata,
        });
        self.post_json("/atheneum/discoveries", &payload).await
    }

    async fn knowledge(&self, target: &str) -> Result<Value> {
        let path = format!("/atheneum/knowledge?target={}", encode(target));
        self.get_json(&path).await
    }

    async fn store_handoff(
        &self,
        from_agent: &str,
        to_agent: &str,
        manifest: &Value,
    ) -> Result<Value> {
        let payload = serde_json::json!({
            "from_agent": from_agent,
            "to_agent": to_agent,
            "manifest": manifest,
        });
        self.post_json("/atheneum/handoffs", &payload).await
    }

    async fn pending_handoff(&self, agent: &str, project: Option<&str>) -> Result<Value> {
        let mut path = format!("/atheneum/handoffs/pending?agent={}", encode(agent));
        if let Some(project) = project {
            path.push_str("&project=");
            path.push_str(&encode(project));
        }
        self.get_json(&path).await
    }

    async fn claim_handoff(&self, id: &str) -> Result<Value> {
        let path = format!("/atheneum/handoffs/{}/claim", encode(id));
        self.post_empty(&path).await
    }

    async fn search(&self, q: &str, k: usize) -> Result<Value> {
        let path = format!("/atheneum/search?q={}&k={}", encode(q), k);
        self.get_json(&path).await
    }

    async fn create_dependency(
        &self,
        blocker: &str,
        dependent: &str,
        reason: &str,
    ) -> Result<Value> {
        let payload = serde_json::json!({
            "blocker_agent": blocker,
            "dependent_agent": dependent,
            "reason": reason,
        });
        self.post_json("/dependencies", &payload).await
    }

    async fn graph_stats(&self) -> Result<Value> {
        self.get_json("/atheneum/graph/stats").await
    }

    async fn heartbeat(&self, agent_id: &str, status: &Value) -> Result<Value> {
        let payload = serde_json::json!({
            "agent_id": agent_id,
            "status": status,
        });
        self.post_json("/heartbeat", &payload).await
    }

    async fn delegate_task(
        &self,
        from_agent: &str,
        to_agent: &str,
        goal: &str,
        context: &str,
    ) -> Result<Value> {
        let manifest = serde_json::json!({
            "goal": goal,
            "context": context,
        });
        let payload = serde_json::json!({
            "from_agent": from_agent,
            "to_agent": to_agent,
            "manifest": manifest,
        });
        self.post_json("/atheneum/handoffs", &payload).await
    }

    async fn declare_workflow(&self, edges: &Value) -> Result<Value> {
        let edges_arr = edges.as_array().ok_or_else(|| {
            anyhow::anyhow!("edges must be a JSON array of {{blocker, dependent, reason}}")
        })?;
        let mut results = Vec::new();
        for edge in edges_arr {
            let blocker = edge["blocker"].as_str().unwrap_or("");
            let dependent = edge["dependent"].as_str().unwrap_or("");
            let reason = edge["reason"].as_str().unwrap_or("");
            let payload = serde_json::json!({
                "blocker": blocker,
                "dependent": dependent,
                "reason": reason,
            });
            match self
                .post_json::<_, serde_json::Value>("/dependencies", &payload)
                .await
            {
                Ok(v) => results.push(serde_json::json!({"edge": edge, "result": v})),
                Err(e) => results.push(serde_json::json!({"edge": edge, "error": e.to_string()})),
            }
        }
        Ok(
            serde_json::json!({"tool": "envoy-mcp", "edges_declared": results.len(), "results": results}),
        )
    }

    async fn status_snapshot(&self, agent: &str) -> Result<Value> {
        let health = self.health().await;
        let agents = self.list_agents().await;
        let handoffs = self.pending_handoff(agent, None).await;

        Ok(serde_json::json!({
            "tool": "envoy-mcp",
            "agent": agent,
            "health": match &health { Ok(v) => v.clone(), Err(e) => serde_json::json!({"error": e.to_string()}) },
            "agents": match &agents { Ok(v) => v.clone(), Err(e) => serde_json::json!({"error": e.to_string()}) },
            "pending_handoffs": match &handoffs { Ok(v) => v.clone(), Err(e) => serde_json::json!({"error": e.to_string()}) },
        }))
    }
}

// -----------------------------------------------------------------------
// Helper: minimal percent-encoding for query/path segments.
// -----------------------------------------------------------------------

fn encode(s: &str) -> String {
    let needs_encoding = s
        .bytes()
        .any(|b| !b.is_ascii_alphanumeric() && !b"-_.~".contains(&b));
    if !needs_encoding {
        return s.to_string();
    }
    s.bytes()
        .map(|b| match b {
            b'-' | b'_' | b'.' | b'~' => String::from_utf8_lossy(&[b]).into_owned(),
            b if b.is_ascii_alphanumeric() => String::from_utf8_lossy(&[b]).into_owned(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_passes_through_safe_strings() {
        assert_eq!(encode("hello-123_world.txt"), "hello-123_world.txt");
    }

    #[test]
    fn encode_escapes_special_chars() {
        assert_eq!(encode("a b&c"), "a%20b%26c");
        assert_eq!(encode("two words"), "two%20words");
    }

    #[test]
    fn base_url_trailing_slash_is_trimmed() {
        let b = HttpBackend::new("http://localhost:9876/");
        assert_eq!(b.base_url, "http://localhost:9876");
    }
}
