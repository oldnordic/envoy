# envoy-mcp

A thin [Model Context Protocol](https://modelcontextprotocol.io/) server that
exposes the [envoy](https://github.com/oldnordic/envoy) daemon's multi-agent
coordination API as MCP tools. All tool calls proxy to the running daemon over
HTTP (default `http://localhost:9876`).

## Tools

15 tools across four groups:

| Group | Tools |
|-------|-------|
| Agent management | `envoy_health`, `envoy_register_agent`, `envoy_list_agents` |
| Messaging | `envoy_send_message`, `envoy_get_messages`, `envoy_ack_message` |
| Discoveries & handoffs | `envoy_store_discovery`, `envoy_knowledge`, `envoy_store_handoff`, `envoy_pending_handoff`, `envoy_claim_handoff` |
| Coordination | `envoy_search`, `envoy_create_dependency`, `envoy_graph_stats`, `envoy_heartbeat` |

## Usage

```sh
# Start (the envoy daemon must already be running on localhost:9876)
ENVOY_URL=http://localhost:9876 envoy-mcp

# Override the daemon URL
ENVOY_URL=http://envoy.internal:9876 envoy-mcp
```

The server speaks MCP over stdio, so it can be wired into any MCP-compatible
client (Claude Desktop, Hermes, Cursor, etc.).

Notable argument details:

- `envoy_register_agent` accepts optional `parent_id` for subagent registration.
- `envoy_pending_handoff` accepts optional `project` to match project-scoped daemon lookups.
- `envoy_heartbeat.status.state` should be one of `working`, `blocked`, `waiting_review`, or `idle`.
