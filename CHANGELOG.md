# Changelog

## 0.1.0 — 2026-05-05

Initial MVP release.

### Engine (initial commit)

- `Engine` wraps sqlitegraph's `SqliteGraph` + `Publisher` for agent coordination
- Channels: `create_channel`, `get_channel`, `get_channel_by_id`, `list_channels`
- Publishing: `publish` with auto-incrementing sequence IDs per channel, stored as graph entities
  with `POSTED_IN` edges
- Replay: `replay(channel, since_seq, limit)` returns events after a sequence cursor
- Subscriptions: `subscribe` seeds `last_seen_sequence` to current max, `catch_up` fetches
  new events and advances the cursor, `unsubscribe` removes the entity and edges
- Stats: `status()` reports channel/event/subscription counts
- Event payloads carry `MagellanTrace` as verifiable proof of code changes

### Dependencies

- axum 0.8 (with WebSocket feature), tokio 1 (full), uuid 1 (v4 + serde),
  tower-http 0.6 (CORS), futures 0.3, tower 0.5 (util)
- Existing: sqlitegraph 2.1.4, rusqlite 0.31 (bundled), serde/serde_json, chrono, thiserror

### Error Types

- 15 error variants: `Graph`, `Serialization`, `ChannelNotFound`, `ChannelAlreadyExists`,
  `NotSubscribed`, `InvalidEntity`, `AgentNotFound`, `AgentOffline`, `AgentAlreadyExists`,
  `MessageNotFound`, `InvalidMessage`, `WsError`, `MessageTooLarge`, `TooManyParts`, `Database`
- `IntoResponse` implementation maps each variant to HTTP status codes and JSON error bodies
  with `code` and `message` fields

### Message Schema

- `MessageEnvelope`: universal message wrapper with `message_id`, `type`, `from`, `to`,
  `task_id`, `context_id`, `timestamp`, `sequence_id`, `parts`
- `Part` / `PartContent`: content parts with `Text`, `Data` (JSON), or `Url` variants
- `MessageType`: `direct`, `handoff`, `heartbeat`, `system`
- `HandoffData`: structured subagent-to-parent handoff with `CompletionStatus` (`DONE`,
  `DONE_WITH_CONCERNS`, `BLOCKED`, `NEEDS_CONTEXT`), `context_remaining_pct`, `what_was_done`,
  `what_is_stubbed`, `remaining_work`, `verification_state`, `magellan_trace`,
  `grounded_queries_used`
- Validation: `BLOCKED` requires `blocked_reason`, `context_remaining_pct` capped at 100,
  max 20 parts per message, max 1 MB text parts

### Agent Registry

- `AgentRegistry`: thread-safe (`Arc<Mutex<AgentTree>>`) in-memory registry
- `register(name, kind, parent_id)`: assigns dot-notation IDs (`id1`, `id1.1`, `id1.1.1`)
  based on hierarchy. Subagents require online parent
- `disconnect(agent_id)`: marks agent and all descendants offline via stack-based traversal
- `get`, `list_all`, `list_online`, `get_children`, `is_online` query methods
- 5 unit tests: root registration, hierarchy, cascade disconnect, offline parent rejection,
  duplicate names

### Message Store

- `MessageStore`: persists messages in SQLite via rusqlite with `envoy_messages` table
- Schema: `id`, `msg_type`, `from_agent`, `to_agent`, `task_id`, `context_id`, `timestamp`,
  `sequence_id`, `parts_json` with index on `(to_agent, sequence_id)`
- `store`: validates, assigns UUID message_id and RFC 3339 timestamp, computes next
  sequence_id via `SELECT COALESCE(MAX(sequence_id), 0)`, inserts row
- `poll(to, since, limit)`: returns messages for recipient after `since` cursor,
  ordered by sequence_id, capped at 100
- `get(message_id)`: single message lookup by ID
- `count_all()`: `SELECT COUNT(*)` from the table

### HTTP Server

- `AppState`: shared state holding `AgentRegistry`, `MessageStore`, `WsRegistry`,
  and server start time
- `build_router(state)`: axum router with 10 routes (agents CRUD, messages send/poll/get,
  health, stats, WebSocket upgrade)
- `POST /agents`: register an agent, returns `201` with `AgentInfo`
- `GET /agents`: list all agents
- `GET /agents/{id}`: agent detail including children IDs
- `DELETE /agents/{id}`: disconnect with cascade, returns affected IDs
- `GET /agents/{id}/messages/pending`: tombstone endpoint for undelivered messages
- `POST /messages`: send message — validates sender online, stores, pushes via WebSocket
  to recipient, returns `201`
- `GET /messages?to=&since=&limit=`: poll with cursor-based pagination, returns
  `PollResponse { messages, latest_sequence }`
- `GET /messages/{id}`: get a single stored message
- `GET /health`: `{ status, uptime_seconds, agents_online }`
- `GET /stats`: `{ messages_total, agents_registered }`
- `GET /ws/{agent_id}`: WebSocket upgrade for real-time event push
- 5 HTTP integration tests: agent registration, list, send/poll, offline rejection, health

### WebSocket Handler

- `WsRegistry`: broadcast-channel registry keyed by agent_id, supporting multiple
  concurrent WebSocket connections per agent
- `ws_handler`: verifies agent is online, upgrades to WebSocket
- `handle_ws`: sends catch-up (all undelivered messages), sends `agent_connected` event,
  enters `tokio::select!` loop pushing broadcast events and receiving client heartbeats
- `send_message` handler pushes stored message to recipient's WebSocket channel after insert
- WebSocket integration test: registers agent, starts server on random port, connects
  via tokio-tungstenite, verifies `agent_connected` event

### Server Binary

- `ENVOY_DB` env var (default: `/home/feanor/.envoy/server.db`)
- `ENVOY_PORT` env var (default: `9876`)
- Automatically creates parent directory for DB path
- `cargo run` starts the server immediately

### Handoff Workflow (End-to-End Test)

- Registers parent agent (Claude) and subagent (`id1.1`)
- Subagent sends handoff with `NeedsContext` status, `context_remaining_pct: 28`,
  verification state (11 passing tests), magellan trace
- Parent polls and retrieves the handoff, verifies all data round-trips correctly
  through the JSON `Data` part
- 19 total tests passing
