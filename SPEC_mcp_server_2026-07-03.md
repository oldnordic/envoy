# SPEC: Envoy MCP Server — Multi-Agent Coordination via MCP

**Date:** 2026-07-03
**Status:** Planning
**Author:** Hermes Agent (per user direction)

## Problem

Envoy is a stateful coordination daemon (sessions, agent registration, messages,
handoffs, discoveries, tasks). It exposes a rich HTTP API (~50 routes across
messaging + atheneum bridge). Currently only Hermes agents can use it (via the
Python `envoy-coordination` plugin). Non-Hermes agents (Claude Code, Cursor,
Codex) have no MCP-native way to coordinate through envoy.

The goal: a thin Rust MCP server using rmcp that proxies to the running envoy
daemon's HTTP API. Same pattern as atheneum-mcp but with an HTTP backend.

## Architecture (separation of concerns preserved)

- `agent-envoy` = the daemon (stateful, HTTP + Unix socket, already running as
  systemd service)
- `envoy-mcp` = thin MCP adapter (rmcp SDK + tool definitions + HTTP client)
  - Does NOT link envoy as a library (envoy is a server, not a lib)
  - Connects to the running daemon via HTTP (localhost:9876) or Unix socket
  - Stateless — all state lives in the daemon

## Crate

`rmcp = { version = "1.7", features = ["server", "transport-io"] }` — same as
atheneum-mcp. Plus `reqwest` for HTTP calls to the daemon.

## Envoy API Surface (verified from source)

### Core Messaging (src/http/router.rs)
| Method | Path | Body |
|--------|------|------|
| GET | /health | — |
| GET | /stats | — |
| GET | /agents | — |
| POST | /agents | `{name, kind, parent_id?}` |
| GET | /agents/{id} | — |
| DELETE | /agents/{id} | — |
| GET | /agents/{id}/messages/pending | — |
| GET | /messages?to=&since=&limit= | — |
| POST | /messages | `{type, from, to, parts, task_id?, context_id?}` |
| GET | /messages/{id} | — |
| POST | /messages/{id}/ack | — |
| POST | /heartbeat | `{agent_id, status}` |
| GET | /agents/{id}/circuit | — |
| POST | /agents/{id}/circuit/failure | — |

### Atheneum Bridge (src/atheneum_bridge/mod.rs — feature-gated)
| Method | Path | Body |
|--------|------|------|
| POST | /atheneum/discoveries | `{agent, discovery_type, target, project_id?, metadata}` |
| GET | /atheneum/discoveries?target= | — |
| POST | /atheneum/handoffs | `{from_agent, to_agent, project_id?, manifest}` |
| GET | /atheneum/handoffs/recent | — |
| GET | /atheneum/handoffs/pending?agent= | — |
| POST | /atheneum/handoffs/{id}/claim | — |
| GET | /atheneum/knowledge?target= | — |
| GET | /atheneum/search?q=&k=&project= | — |
| GET | /atheneum/context | — |
| POST/GET | /atheneum/tasks | — |
| GET | /atheneum/sessions | — |
| GET | /atheneum/events | — |
| GET | /atheneum/graph/stats | — |
| GET | /atheneum/graph/navigate?q=&k=&depth= | — |

### Dependencies (task coordination)
| Method | Path | Body |
|--------|------|------|
| POST | /dependencies | `{blocker, dependent, reason}` |
| GET | /dependencies/blocking/{id} | — |
| GET | /dependencies/blocked-by/{id} | — |
| POST | /dependencies/{id}/resolve | — |

### Tasks (envoy-native)
| Method | Path | Body |
|--------|------|------|
| POST | /tasks/propose | — |
| POST | /tasks/claim-next | — |
| POST | /tasks/{id}/claim | — |
| POST | /tasks/{id}/state | — |
| GET | /tasks/{id} | — |
| GET | /tasks | — |

### Request Types (verified from source)
- `RegisterRequest { name: String, parent_id: Option<String>, kind: String }`
- `SendMessageRequest { type: MessageType, from: String, to: String, task_id: Option<String>, context_id: Option<String>, parts: Vec<Part> }`
- `StoreDiscoveryRequest { agent, discovery_type, target, project_id?, metadata: Value }`
- `StoreHandoffRequest { from_agent, to_agent, project_id?, manifest: Value }`
- `MessageType = Direct | Handoff | Heartbeat | System`
- `Part { content: PartContent }` (flattened — `{"type": "text", "text": "..."}`)

## Target Tool Surface (15 tools)

### Group 1: AGENT MANAGEMENT (3)
| Tool | HTTP call | Description |
|------|-----------|-------------|
| `envoy_health` | GET /health | Check daemon liveness |
| `envoy_register_agent` | POST /agents | Register an agent (name, kind) |
| `envoy_list_agents` | GET /agents | List all registered agents |

### Group 2: MESSAGING (3)
| Tool | HTTP call | Description |
|------|-----------|-------------|
| `envoy_send_message` | POST /messages | Send a message to another agent |
| `envoy_get_messages` | GET /messages | Poll messages for an agent |
| `envoy_ack_message` | POST /messages/{id}/ack | Acknowledge a message |

### Group 3: DISCOVERIES & HANDOFFS (5)
| Tool | HTTP call | Description |
|------|-----------|-------------|
| `envoy_store_discovery` | POST /atheneum/discoveries | Store a finding in the graph |
| `envoy_knowledge` | GET /atheneum/knowledge | Query aggregated knowledge for a target |
| `envoy_store_handoff` | POST /atheneum/handoffs | Create a pending task handoff |
| `envoy_pending_handoff` | GET /atheneum/handoffs/pending | Get pending handoffs for an agent |
| `envoy_claim_handoff` | POST /atheneum/handoffs/{id}/claim | Claim a handoff |

### Group 4: COORDINATION (4)
| Tool | HTTP call | Description |
|------|-----------|-------------|
| `envoy_search` | GET /atheneum/search | Search the knowledge graph |
| `envoy_create_dependency` | POST /dependencies | Declare a task dependency |
| `envoy_graph_stats` | GET /atheneum/graph/stats | Graph topology stats |
| `envoy_heartbeat` | POST /heartbeat | Send a heartbeat |

## Phased Implementation

### Phase 1: Create envoy-mcp crate skeleton
**Scope:** New workspace member `crates/envoy-mcp/` with Cargo.toml, main.rs,
lib.rs. Copy the structural pattern from atheneum-mcp (ServerHandler, ToolRouter)
but with an HTTP-only backend (no direct/library mode — envoy is a server).

**Changes:**
- `crates/envoy-mcp/Cargo.toml`: rmcp + reqwest + tokio + serde + anyhow + tracing
- `crates/envoy-mcp/src/main.rs`: read `ENVOY_URL` env (default
  `http://localhost:9876`), create HttpBackend, serve via stdio
- `crates/envoy-mcp/src/lib.rs`: EnvoyMcpServer struct + ServerHandler impl
- `crates/envoy-mcp/src/backend.rs`: HttpBackend with reqwest client, GET/POST
  helpers, all 15 methods
- `crates/envoy-mcp/src/tools.rs`: 15 tool definitions
- Add to workspace `Cargo.toml` members

**Verify:** `cargo build -p envoy-mcp` compiles. Server starts and connects to
the running daemon.

### Phase 2: Implement HttpBackend (all 15 methods)
**Scope:** Each method makes a real HTTP call to the running daemon. No stubs.

**Verify:** Manual curl-equivalent tests against the live daemon.

### Phase 3: Register 15 tools + integration tests
**Scope:** Tool definitions with JSON Schema. Integration test: mock HTTP
server or real daemon round-trip.

**Verify:** `cargo test -p envoy-mcp` passes.

### Phase 4: Install + wire to .mcp.json + Hermes config
**Scope:** Build release, install to ~/.local/bin, add to .mcp.json + Hermes
config.yaml.

**Verify:** MCP tools appear and work.

## Quality gates
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets -p envoy-mcp -- -D warnings`
- `cargo test -p envoy-mcp`
- `cargo build --release -p envoy-mcp`
- E2E: server connects to live daemon, health returns ok
