# Envoy — Message/Coordination Server for AI Coding Agents

HTTP+JSON coordination server built on [sqlitegraph](https://crates.io/crates/sqlitegraph).
Replaces file-based message passing with real-time structured messaging, agent identity
management, and a subagent handoff protocol.

## Quick Start

```bash
# Build
cargo build

# Run (defaults: 127.0.0.1:9876, db at ~/.envoy/server.db)
cargo run

# Custom port and database
ENVOY_PORT=9876 ENVOY_DB=/path/to/envoy.db cargo run

# Run tests
cargo test
```

## What It Does

Envoy sits between AI coding agents (Claude, Hermes, subagents) and provides:

- **Agent registry** — Agents register to get server-assigned IDs with parent/child
  hierarchy. Subagents get dot-notation IDs (`id1.1`, `id1.1.1`).
- **Direct messaging** — Agents send structured messages to each other via HTTP.
  Messages get sequence IDs and are persisted in SQLite.
- **Real-time push** — WebSocket connections push messages to connected agents
  instantly, with catch-up delivery on connect.
- **Handoff protocol** — Subagents hand work back to parents with structured data:
  completion status, context remaining, what was done/stubbed, verification state,
  and a magellan trace proving what code changed.
- **Cascade disconnect** — Disconnecting a parent agent marks all descendants
  offline. Undelivered messages are preserved as tombstones.

## Architecture

```
Agent (Claude) ──HTTP POST /messages──▶  ┌──────────────┐  ──WebSocket push──▶  Agent (Hermes)
Agent (Sub)    ──handoff message─────▶  │    envoy      │  ──poll GET────────▶  Agent (Parent)
                                        │  SQLite DB     │
                                        └──────────────┘
```

```
src/
├── lib.rs          # Public API re-exports
├── main.rs         # Binary entry point
├── error.rs        # EnvoyError enum + IntoResponse
├── types.rs        # Channel, Event, EventPayload, Subscription, EngineStats
├── engine.rs       # Core pub/sub engine (wraps sqlitegraph)
├── agent.rs        # AgentRegistry with parent/child hierarchy
├── message.rs      # MessageEnvelope, Part, HandoffData, MessageStore
├── http.rs         # Axum HTTP + WebSocket handlers, AppState, WsRegistry
└── server.rs       # Server startup (DB open + axum::serve)
```

### Key Types

| Type | Purpose |
|------|---------|
| `MessageEnvelope` | Universal message wrapper: type, from, to, task_id, context_id, sequence_id, parts |
| `Part` / `PartContent` | Content parts: `Text`, `Data` (JSON), or `Url` |
| `HandoffData` | Subagent-to-parent handoff: completion status, context %, verification state, magellan trace |
| `MessageType` | `direct`, `handoff`, `heartbeat`, `system` |
| `AgentInfo` | Agent identity: agent_id, name, kind, parent_id, online |
| `CompletionStatus` | `DONE`, `DONE_WITH_CONCERNS`, `BLOCKED`, `NEEDS_CONTEXT` |

### Message Limits

| Limit | Value |
|-------|-------|
| Max parts per message | 20 |
| Max text part size | 1 MB |
| Max poll limit | 100 |
| WebSocket broadcast channel | 256 messages |

## API Overview

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/agents` | Register a new agent |
| `GET` | `/agents` | List all agents |
| `GET` | `/agents/{id}` | Get agent detail with children |
| `DELETE` | `/agents/{id}` | Disconnect agent (cascades) |
| `GET` | `/agents/{id}/messages/pending` | Tombstone: undelivered messages |
| `POST` | `/messages` | Send a message |
| `GET` | `/messages?to=&since=&limit=` | Poll messages for recipient |
| `GET` | `/messages/{id}` | Get a single message |
| `GET` | `/ws/{agent_id}` | WebSocket upgrade |
| `GET` | `/health` | Health check + uptime |
| `GET` | `/stats` | Message count + agent count |

Full API reference: [API.md](API.md)

## Configuration

| Env Var | Default | Purpose |
|---------|---------|---------|
| `ENVOY_DB` | `/home/feanor/.envoy/server.db` | SQLite database path |
| `ENVOY_PORT` | `9876` | HTTP listen port |

## Requirements

- Rust 1.82+
- SQLite (bundled via rusqlite)

## License

GPL-3.0
