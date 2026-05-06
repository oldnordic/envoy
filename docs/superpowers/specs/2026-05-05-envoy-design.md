# Envoy — Coordination Engine Design Spec

**Version:** 0.1.0 (MVP)
**Date:** 2026-05-05
**Status:** Design approved — ready for implementation planning

## Overview

Envoy is an HTTP+JSON coordination server for AI coding agents, built on sqlitegraph's pub/sub graph database. It replaces file-based markdown message passing with real-time structured messaging, agent identity management, and a formal subagent handoff protocol.

**Problem solved:** AI coding subagents degrade under context-window pressure, stubbing work and reporting "DONE" when they haven't actually finished. Envoy provides proactive handoff signaling so subagents can transfer state before quality collapses, plus real-time push delivery so no agent polls stale folders.

## MVP Scope

| Feature | Phase |
|---------|-------|
| Agent registration with parent/child hierarchy | 1 (MVP) |
| Direct messages (agent → agent) | 1 (MVP) |
| WebSocket push for real-time delivery | 1 (MVP) |
| Structured handoff message type | 1 (MVP) |
| Channels, broadcast pub/sub | 2 |
| Replay/catch-up, agent cards, discovery | 2 |

## Agent Identity Model

Dot-notation hierarchy, envoy-assigned IDs (clients cannot mint IDs).

```
id1 (claude)
  ├── id1.1 (claude/subagent: implement-task-3)
  │   └── id1.1.1 (claude/subagent: fix-compilation)
  └── id1.2 (claude/subagent: code-review)
id2 (hermes)
  └── id2.1 (hermes/subagent: schema-migration)
```

Rules:
- `parent_id` null = root agent
- `kind` = agent platform (claude, hermes, codex, opencode, openclaw)
- Name is a label (non-unique), agent_id is identity (unique)
- Re-registration after disconnect = NEW id, old id tombstoned
- On disconnect, all descendants cascade-offline with WebSocket notification
- Tombstone endpoint (`GET /agents/{id}/messages/pending`) for undelivered messages

## Message Model

### Envelope (all messages)

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `message_id` | UUID string | Yes | Envoy-assigned |
| `type` | enum | Yes | `direct`, `handoff`, `heartbeat`, `system` |
| `from` | string | Yes | Sender agent_id |
| `to` | string | Yes | Recipient agent_id |
| `task_id` | UUID string | No | Unit of work |
| `context_id` | UUID string | No | Conversation thread |
| `timestamp` | ISO 8601 | Yes | Server-assigned |
| `sequence_id` | i64 | Yes | Monotonic per recipient |
| `parts` | array of Part | Yes | Message content |

### Part Types

Each part has exactly one content variant:

| Variant | Type | Purpose |
|---------|------|---------|
| `text` | string | Human-readable prose |
| `data` | arbitrary JSON | Structured data (traces, verification state) |
| `url` | string | External resource reference |

### Handoff Part Schema (`type: "handoff"`, `part.data`)

```json
{
  "completion_status": "DONE | DONE_WITH_CONCERNS | BLOCKED | NEEDS_CONTEXT",
  "blocked_reason": "optional — required when status is BLOCKED",
  "context_remaining_pct": 28,
  "what_was_done": [
    {"scope": "src/file.rs", "change": "description", "verified": true}
  ],
  "what_is_stubbed": [
    {"location": "src/file.rs", "reason": "why left incomplete"}
  ],
  "remaining_work": ["task 1", "task 2"],
  "verification_state": {
    "tests_passing": 11,
    "tests_failing": 0,
    "quality_gate": {"passed": true, "blocking": 0, "warnings": 2},
    "cargo_check_passed": true
  },
  "magellan_trace": {
    "files_changed": ["..."],
    "symbols_added": ["..."],
    "symbols_removed": ["..."],
    "refs_in": {"symbol": 3},
    "refs_out": {"symbol": 5}
  },
  "grounded_queries_used": [
    "magellan find --name Engine",
    "llmgrep search --query 'handoff protocol'"
  ]
}
```

## API

### Agents

```
POST   /agents/register         201  Register agent, get ID
DELETE /agents/{agent_id}       200  Disconnect (cascade descendants)
GET    /agents                  200  List all agents
GET    /agents/{agent_id}       200  Agent info + children
GET    /agents/{agent_id}/messages/pending  200  Tombstone — undelivered messages
```

### Messages

```
POST   /messages                201  Send message (direct or handoff)
GET    /messages?to={id}&since={seq}&limit={n}  200  Poll for new messages
GET    /messages/{message_id}   200  Get specific message
```

### WebSocket

```
WS /ws/{agent_id}
  Events: message, handoff, agent_connected, agent_disconnected, error
```

### Health

```
GET /health  200  Server status + uptime + agents online
GET /stats   200  Messages total, agents registered, handoffs count
```

### Error Format

```json
{ "error": { "code": "AGENT_NOT_FOUND", "message": "..." } }
```

Status codes: 400 (bad request), 404 (not found), 409 (agent offline/collision), 500 (internal).

## Server Architecture

```
envoy binary (Rust)
├── HTTP layer (axum)
│   ├── REST handlers — agents, messages
│   └── WebSocket handler — per-agent push
├── Engine (existing — SqliteGraph + Publisher)
│   ├── Persistence — all messages stored as graph entities
│   └── In-process pub/sub — NodeChanged events on writes
└── Agent Registry (new)
    ├── Agent tree — parent/child hierarchy
    ├── Connection state — online/offline tracking
    └── Tombstone forwarding — undelivered message recovery
```

## Database

- **Location:** `/home/feanor/.envoy/server.db` (NOTE: update CLAUDE.md which currently says `.magellan/envoy.db`)
- **Engine:** sqlitegraph >= 2.1.4 (crates.io)
- Entities stored as graph nodes with edges for relationships
- Kind constants: `EnvoyAgent`, `EnvoyMessage`, `EnvoyPart`

## Operational Semantics

### Sequence ID Persistence

Sequence IDs survive server restarts — they are stored in the database, not in memory. On startup, envoy reads `max(sequence_id)` per recipient from the message log and resumes from there. No reset.

### WebSocket Reconnection

When an agent reconnects via WebSocket after a disconnect (same agent_id, re-authenticated):

1. Envoy sends all messages with `sequence_id > agent.last_delivered_seq` as a batch
2. Then resumes real-time push for new messages
3. Agent can also poll `GET /messages?to={id}&since={last_seen}` for explicit catch-up

If the agent registers with a NEW agent_id (disconnected + re-registered), it gets no historical messages — old messages are accessible only via the tombstone endpoint.

### Message Limits

| Limit | Value | Rationale |
|-------|-------|-----------|
| Max message body size | 1 MB | Prevents memory exhaustion from runaway subagents |
| Max parts per message | 20 | Reasonable ceiling for structured content |
| Max agents in tree | 1000 | Prevents unbounded registration |
| Max tree depth | 10 | Practical limit far above realistic use (~3) |

Limits return 400 with descriptive error. Not configurable in MVP.

### Handoff BLOCKED Semantics

When `completion_status` is `BLOCKED`, the `blocked_reason` field is required. The parent agent receives this in real-time via WebSocket and can:
1. Provide the missing context (send a direct message back)
2. Escalate to a more capable model
3. Break the task into smaller pieces
4. Take over the work itself

## Existing Code Mapping

Current source files:
- `src/lib.rs` — Public API, re-exports
- `src/engine.rs` — Core Engine (SqliteGraph + Publisher)
- `src/types.rs` — Data types (Channel, Event, EventPayload, etc.)
- `src/error.rs` — Error types

New modules for MVP:
- `src/agent.rs` — Agent registry, tree management, connection state
- `src/http.rs` — Axum HTTP server, REST handlers, WebSocket handler
- `src/message.rs` — Message envelope, parts, routing, handoff serialization
- `src/server.rs` — Binary entry point, server lifecycle, config

## Error Handling

- `Result<T, EnvoyError>` throughout — no bare unwrap/expect
- EnvoyError variants: Graph, Serialization, AgentNotFound, AgentOffline, AgentAlreadyExists, MessageNotFound, InvalidMessage, WsError
- All sqlitegraph errors map through `EnvoyError::Graph`

## Testing Strategy

- Unit tests: engine operations, agent registry, message routing, handoff serialization
- Integration tests: full HTTP round-trip (register → send → poll → WebSocket receive), handoff flow (subagent registers → sends handoff → parent receives), disconnect cascade (parent disconnects → children offline), tombstone forwarding
- WebSocket tests: connection lifecycle, event delivery, error propagation

## Design Decisions Log

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Transport | HTTP + JSON + WebSocket | Simple, testable with curl, push without polling |
| ID assignment | Server-only | Prevents collisions across agent instances |
| Name uniqueness | Not enforced | Name is label, ID is identity; multi-instance setups |
| contextId + taskId | Both from start | Separate concerns (thread vs unit of work) |
| MVP scope | Registration + messages + WS + handoff | Hermes feedback: ship minimal, layer features |
| DB location | `~/.envoy/server.db` | Infrastructure, not project data |
| A2A adaptation | Steal data model, skip gRPC/Protobuf | Right abstractions, wrong transport weight |

## References

- [A2A Protocol v1.0.0 Specification](https://a2a-protocol.org/latest/specification/)
- [Agentic Coding Patterns — Handoff](https://aipatternbook.com/handoff)
- [Claude Code Issue #40339 — Subagent Delegation Quality](https://github.com/anthropics/claude-code/issues/40339)
- [Zylos Research — Context Window Management](https://zylos.ai/research/2026-03-31-context-window-management-session-lifecycle-long-running-agents)
- [Chroma Research — Context Rot](https://research.trychroma.com/context-rot)
