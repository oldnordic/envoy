//! MCP tool definitions for Envoy.
//!
//! Tools are registered manually via [`ToolRoute::new_dyn`] so we control the
//! input schemas directly as JSON Schema objects — no `schemars` dependency
//! is required.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolResult, Content, JsonObject, Tool};
use rmcp::ErrorData as McpError;
use serde_json::{json, Map, Value};

use crate::EnvoyMcpServer;

// -----------------------------------------------------------------------
// Registration
// -----------------------------------------------------------------------

pub fn register_all(router: &mut ToolRouter<EnvoyMcpServer>) {
    router.add_route(envoy_health());
    router.add_route(envoy_register_agent());
    router.add_route(envoy_list_agents());
    router.add_route(envoy_send_message());
    router.add_route(envoy_get_messages());
    router.add_route(envoy_ack_message());
    router.add_route(envoy_store_discovery());
    router.add_route(envoy_knowledge());
    router.add_route(envoy_store_handoff());
    router.add_route(envoy_pending_handoff());
    router.add_route(envoy_claim_handoff());
    router.add_route(envoy_search());
    router.add_route(envoy_create_dependency());
    router.add_route(envoy_graph_stats());
    router.add_route(envoy_heartbeat());
    // Composite tools
    router.add_route(delegate_task());
    router.add_route(declare_workflow());
    router.add_route(status_snapshot());
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

/// Build a successful CallToolResult from a JSON value (pretty-printed text).

fn schema_obj(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn json_result(value: Value) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(&value)
        .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

fn extract_args(args: Option<JsonObject>) -> Value {
    Value::Object(args.unwrap_or_default())
}

/// Shared type alias for the boxed async tool route closure.
type Route = rmcp::handler::server::router::tool::ToolRoute<EnvoyMcpServer>;

// -----------------------------------------------------------------------
// Group 1: Agent management
// -----------------------------------------------------------------------

fn envoy_health() -> Route {
    let schema = json!({ "type": "object", "properties": {} });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_health",
            "Check whether the envoy daemon is alive and responsive.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                match ctx.service.backend.health().await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_register_agent() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "Human-readable agent name" },
            "kind": { "type": "string", "description": "Agent kind (e.g. claude, codex, hermes, cursor)" },
            "parent_id": { "type": "string", "description": "Optional parent agent id for subagent registration." }
        },
        "required": ["name", "kind"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_register_agent",
            "Register a new agent with the envoy daemon.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let name = args["name"].as_str().unwrap_or("").to_string();
                let kind = args["kind"].as_str().unwrap_or("").to_string();
                let parent_id = args.get("parent_id").and_then(|v| v.as_str());
                match ctx
                    .service
                    .backend
                    .register_agent(&name, &kind, parent_id)
                    .await
                {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_list_agents() -> Route {
    let schema = json!({ "type": "object", "properties": {} });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_list_agents",
            "List all agents currently registered with the daemon.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                match ctx.service.backend.list_agents().await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

// -----------------------------------------------------------------------
// Group 2: Messaging
// -----------------------------------------------------------------------

fn envoy_send_message() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "type": { "type": "string", "enum": ["direct", "handoff", "heartbeat", "system"], "description": "Message type" },
            "from": { "type": "string", "description": "Sender agent id" },
            "to": { "type": "string", "description": "Recipient agent id" },
            "parts": {
                "type": "array",
                "items": { "type": "object" },
                "description": "Message parts, e.g. [{\"type\":\"text\",\"text\":\"hello\"}]"
            }
        },
        "required": ["type", "from", "to", "parts"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_send_message",
            "Send a message to another agent via the daemon.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let msg_type = args["type"].as_str().unwrap_or("direct").to_string();
                let from = args["from"].as_str().unwrap_or("").to_string();
                let to = args["to"].as_str().unwrap_or("").to_string();
                let parts = args["parts"].clone();
                match ctx
                    .service
                    .backend
                    .send_message(&msg_type, &from, &to, &parts)
                    .await
                {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_get_messages() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "to": { "type": "string", "description": "Agent id to poll messages for" },
            "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 50, "description": "Max messages to return" }
        },
        "required": ["to"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_get_messages",
            "Poll messages addressed to an agent.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let to = args["to"].as_str().unwrap_or("").to_string();
                let limit = args["limit"].as_i64().unwrap_or(50);
                match ctx.service.backend.get_messages(&to, limit).await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_ack_message() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "Message id to acknowledge" }
        },
        "required": ["id"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_ack_message",
            "Acknowledge that an agent has received/processed a message.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let id = args["id"].as_str().unwrap_or("").to_string();
                match ctx.service.backend.ack_message(&id).await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

// -----------------------------------------------------------------------
// Group 3: Discoveries & handoffs
// -----------------------------------------------------------------------

fn envoy_store_discovery() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "agent": { "type": "string", "description": "Agent recording the discovery" },
            "discovery_type": { "type": "string", "description": "Type (Decision, Bug, Finding, Pattern, etc.)" },
            "target": { "type": "string", "description": "Entity or topic being observed" },
            "metadata": { "type": "object", "description": "Free-form metadata about the discovery" }
        },
        "required": ["agent", "discovery_type", "target", "metadata"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_store_discovery",
            "Store a discovery into the shared knowledge graph.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let agent = args["agent"].as_str().unwrap_or("").to_string();
                let dtype = args["discovery_type"].as_str().unwrap_or("").to_string();
                let target = args["target"].as_str().unwrap_or("").to_string();
                let metadata = args["metadata"].clone();
                match ctx
                    .service
                    .backend
                    .store_discovery(&agent, &dtype, &target, &metadata)
                    .await
                {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_knowledge() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "target": { "type": "string", "description": "Entity name to query aggregated knowledge for" }
        },
        "required": ["target"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_knowledge",
            "Query aggregated knowledge about a target entity.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let target = args["target"].as_str().unwrap_or("").to_string();
                match ctx.service.backend.knowledge(&target).await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_store_handoff() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "from_agent": { "type": "string", "description": "Agent handing off the task" },
            "to_agent": { "type": "string", "description": "Agent receiving the handoff" },
            "manifest": { "type": "object", "description": "Handoff manifest (context, remaining work, verification state)" }
        },
        "required": ["from_agent", "to_agent", "manifest"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_store_handoff",
            "Create a pending task handoff between two agents.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let from_agent = args["from_agent"].as_str().unwrap_or("").to_string();
                let to_agent = args["to_agent"].as_str().unwrap_or("").to_string();
                let manifest = args["manifest"].clone();
                match ctx
                    .service
                    .backend
                    .store_handoff(&from_agent, &to_agent, &manifest)
                    .await
                {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_pending_handoff() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "agent": { "type": "string", "description": "Agent id to fetch pending handoffs for" },
            "project": { "type": "string", "description": "Optional project filter for project-scoped handoffs" }
        },
        "required": ["agent"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_pending_handoff",
            "Get pending handoffs addressed to an agent.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let agent = args["agent"].as_str().unwrap_or("").to_string();
                let project = args.get("project").and_then(|v| v.as_str());
                match ctx.service.backend.pending_handoff(&agent, project).await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_claim_handoff() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "Handoff id to claim" }
        },
        "required": ["id"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_claim_handoff",
            "Claim a pending handoff for the calling agent.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let id = args["id"].as_str().unwrap_or("").to_string();
                match ctx.service.backend.claim_handoff(&id).await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

// -----------------------------------------------------------------------
// Group 4: Coordination
// -----------------------------------------------------------------------

fn envoy_search() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "q": { "type": "string", "description": "Search query" },
            "k": { "type": "integer", "minimum": 1, "maximum": 100, "default": 10, "description": "Number of results" }
        },
        "required": ["q"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_search",
            "Lexical search over the shared knowledge graph.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let q = args["q"].as_str().unwrap_or("").to_string();
                let k = args["k"].as_u64().unwrap_or(10) as usize;
                match ctx.service.backend.search(&q, k).await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_create_dependency() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "blocker": { "type": "string", "description": "Agent that must finish first" },
            "dependent": { "type": "string", "description": "Agent that is blocked/wating" },
            "reason": { "type": "string", "description": "Why the dependency exists" }
        },
        "required": ["blocker", "dependent", "reason"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_create_dependency",
            "Declare a task dependency between two agents.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let blocker = args["blocker"].as_str().unwrap_or("").to_string();
                let dependent = args["dependent"].as_str().unwrap_or("").to_string();
                let reason = args["reason"].as_str().unwrap_or("").to_string();
                match ctx
                    .service
                    .backend
                    .create_dependency(&blocker, &dependent, &reason)
                    .await
                {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_graph_stats() -> Route {
    let schema = json!({ "type": "object", "properties": {} });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_graph_stats",
            "Return high-level knowledge graph statistics.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                match ctx.service.backend.graph_stats().await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn envoy_heartbeat() -> Route {
    let schema = json!({
        "type": "object",
        "properties": {
            "agent_id": { "type": "string", "description": "Agent id sending the heartbeat" },
            "status": {
                "type": "object",
                "description": "Optional status snapshot (state, working_on, etc.)",
                "properties": {
                    "state": {
                        "type": "string",
                        "enum": ["working", "blocked", "waiting_review", "idle"],
                        "description": "Workflow state accepted by the daemon."
                    },
                    "task_id": { "type": "string" },
                    "blocked_reason": { "type": "string" },
                    "waiting_on_agent": { "type": "string" },
                    "checkpoint": { "type": "string" },
                    "working_on": { "type": "string" }
                }
            }
        },
        "required": ["agent_id"]
    });
    let schema: Map<String, Value> = schema.as_object().unwrap().clone();
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "envoy_heartbeat",
            "Send a heartbeat (and optional status) to the daemon.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let agent_id = args["agent_id"].as_str().unwrap_or("").to_string();
                let status = args.get("status").cloned().unwrap_or(json!({}));
                match ctx.service.backend.heartbeat(&agent_id, &status).await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

// -----------------------------------------------------------------------
// Composite tools
// -----------------------------------------------------------------------

fn delegate_task() -> Route {
    let schema = schema_obj(json!({
        "type": "object",
        "properties": {
            "from_agent": { "type": "string", "description": "Agent delegating the task" },
            "to_agent": { "type": "string", "description": "Agent receiving the task" },
            "goal": { "type": "string", "description": "What the receiving agent should accomplish" },
            "context": { "type": "string", "description": "Background context for the task (file paths, prior findings, constraints)" }
        },
        "required": ["from_agent", "to_agent", "goal"]
    }));
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "delegate_task",
            "Delegate a task to another agent via a persistent handoff. \
             Creates the handoff and returns immediately — the receiving agent \
             picks it up when ready. Survives agent restarts.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let from = args["from_agent"].as_str().unwrap_or("").to_string();
                let to = args["to_agent"].as_str().unwrap_or("").to_string();
                let goal = args["goal"].as_str().unwrap_or("").to_string();
                let context = args["context"].as_str().unwrap_or("").to_string();
                match ctx
                    .service
                    .backend
                    .delegate_task(&from, &to, &goal, &context)
                    .await
                {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn declare_workflow() -> Route {
    let schema = schema_obj(json!({
        "type": "object",
        "properties": {
            "edges": {
                "type": "array",
                "description": "Dependency edges. Each edge: {blocker, dependent, reason}. \
                                Envoy enforces that blocker finishes before dependent starts.",
                "items": {
                    "type": "object",
                    "properties": {
                        "blocker": { "type": "string", "description": "Agent that must finish first" },
                        "dependent": { "type": "string", "description": "Agent that is blocked/waiting" },
                        "reason": { "type": "string", "description": "Why the dependency exists" }
                    },
                    "required": ["blocker", "dependent", "reason"]
                }
            }
        },
        "required": ["edges"]
    }));
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "declare_workflow",
            "Declare a multi-agent workflow as a dependency graph. \
             Each edge says blocker must finish before dependent starts. \
             Envoy enforces ordering and tracks completion across restarts.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let edges = args.get("edges").cloned().unwrap_or(json!([]));
                match ctx.service.backend.declare_workflow(&edges).await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}

fn status_snapshot() -> Route {
    let schema = schema_obj(json!({
        "type": "object",
        "properties": {
            "agent": { "type": "string", "description": "Agent ID to check pending handoffs for" }
        },
        "required": ["agent"]
    }));
    rmcp::handler::server::router::tool::ToolRoute::new_dyn(
        Tool::new(
            "status_snapshot",
            "Dashboard view: daemon health + all agents + pending handoffs for the caller. \
             One call replaces three. Use to check who is online, what work is waiting.",
            schema,
        ),
        |ctx: rmcp::handler::server::tool::ToolCallContext<'_, EnvoyMcpServer>| {
            Box::pin(async move {
                let args = extract_args(ctx.arguments);
                let agent = args["agent"].as_str().unwrap_or("").to_string();
                match ctx.service.backend.status_snapshot(&agent).await {
                    Ok(v) => json_result(v),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            })
        },
    )
}
