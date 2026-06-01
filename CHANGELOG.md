# Changelog

## [Unreleased]

### Added

- **`POST /atheneum/events`** — Generic event logging endpoint. Accepts `session_id`, `event_type`, `entity_id`, and arbitrary JSON `payload`. Persists to the `event_log` table for cross-session auditing.
- **`GET /atheneum/sessions`** now supports **cross-project queries**. The `project` query parameter is now optional (`Option<String>`). When omitted, returns sessions from all projects.
- **`GET /atheneum/events`** — Query generic events. Supports filtering by `session_id`, `event_type`, and `limit`.
- **Graph navigation endpoints** — New `GET /atheneum/graph/*` routes:
  - `GET /atheneum/graph/entities/{id}` — read entity by ID
  - `GET /atheneum/graph/edges/{id}` — read edge by ID
  - `GET /atheneum/graph/entities/{id}/neighbors?depth=N` — one-hop edges (depth=0) or BFS subgraph (depth>0)
  - `GET /atheneum/graph/navigate?query=X[&k=N&depth=D&project=P]` — semantic search + graph walk (the primary LLM tool)
  - `GET /atheneum/graph/stats` — topological summary (entity + edge counts by kind/type)
- **Semantic search auto-index** — `GET /atheneum/search` no longer rebuilds the HNSW index on every request. Discoveries are auto-indexed on write in `store_discovery()`.

### Fixed

- **CI and doc monitors now env-gated** — `ENVOY_CI_MONITOR=project,owner/repo,interval` and `ENVOY_DOC_MONITOR=project,repo_path,interval`. Both are off by default; previously hardcoded to poll `oldnordic/magellan` CI and watch `.` on every startup.
- **Default DB path uses `$XDG_DATA_HOME`/`$HOME`** — no longer hardcoded to `/home/feanor/.envoy/server.db`.
- **`build_router_unlimited()` includes atheneum routes** — test helper now correctly mounts bridge routes when the `atheneum` feature is active. Fixes 3 failing integration tests.
- **Removed `dashboard` feature** — `dashboard.rs` (498 LOC) was never compiled or routed. Feature flag, source file, and 3 test files deleted.
- **SQL parameter ordering bug in `query_sessions`** — Fixed a mismatch where `parent_id` parameters were incorrectly placed when `project` was `None` but `parent_id` was `Some`, causing runtime SQLite parameter count errors.
- **CI workflow dependency stripping** — `sed` command now correctly comments out both `[dependencies]` and `[dev-dependencies]` `atheneum` path entries to prevent "optional dependency not included in any feature" error during GitHub Actions.

### Agent Registry (Breaking)

- **Idempotent registration** — `POST /agents` with the same name no longer creates duplicates. If an active agent with that name exists, the existing agent is returned with `is_new: false` and HTTP 200.
- **Retired ID reuse pool** — When an agent is explicitly retired (via `DELETE /agents/{id}`), its numeric ID is added to a reuse pool. New registrations reuse the lowest available retired ID instead of incrementing forever. This prevents ID exhaustion in long-running deployments.
- **Server restart lifecycle** — On restart, all agents loaded from the database start as `Retired`. They must re-register or send a heartbeat to become `Active` again. Only agents that were explicitly retired before shutdown have their IDs added to the reuse pool; agents that were simply offline due to restart keep their IDs reserved.
- **Server-assigned identification** — Registration response now includes:
  - `agent_id` — the canonical ID to use for all future requests
  - `is_new` — `true` if created, `false` if returning existing
  - `message` — explicit instruction: "Use agent_id 'X' for all future requests. Include it in the x-agent-id header."

### Architecture

- **Split `src/http.rs` (1968 LOC) into focused modules** — `src/http/` directory:
  - `state.rs` — `AppState`, `SharedState`, `WsRegistry`, `recover_lock`, `run_nudge_loop`
  - `middleware.rs` — `rate_limit_middleware`
  - `router.rs` — `build_router`, `build_router_unlimited`, `build_base_routes`
  - `types.rs` — request/response structs (`RegisterRequest`, `SendMessageRequest`, etc.)
  - `handlers/` — 8 focused handler modules split from monolithic `handlers.rs`:
    - `agents.rs` — agent CRUD handlers (`register_agent`, `disconnect_agent`, etc.)
    - `messages.rs` — message handlers (`send_message`, `poll_messages`, etc.)
    - `diagnostics.rs` — health and stats handlers
    - `circuits.rs` — circuit breaker and dependency handlers
    - `audit.rs` — event ingestion and audit query handlers
    - `tasks.rs` — task CRUD handlers
    - `subscriptions.rs` — project subscription handlers
    - `project.rs` — project config handlers
    - `mod.rs` — re-exports preserving `router.rs` wildcard import compatibility
  - `ws.rs` — WebSocket handler and `broadcast_to_project`
  - All existing public API preserved via `mod.rs` re-exports.

- **Split `src/atheneum_bridge.rs` (~2400 LOC) into focused modules** — `src/atheneum_bridge/` directory:
  - `utils.rs` — Shared helpers (`entity_to_json`, `parse_status`, `parse_blocker_type`, `default_*`)
  - `types.rs` — All request/response structs (`SearchRequest`, `IngestRequest`, `BlockerRequest`, etc.)
  - `discovery.rs` — Discovery CRUD handlers
  - `tasks.rs` — Task/kanban handlers
  - `actions.rs` — Blocker/action handlers
  - `ontology.rs` — Ontology/graph handlers
  - `import.rs` — Import/ingest handlers
  - `sessions.rs` — Session recording handlers
  - All existing public API preserved via `mod.rs` re-exports.

## 0.3.0 — 2026-05-09

Atheneum integration — cross-agent knowledge sharing and handoff protocol.

### Atheneum HTTP Routes (Phase 4)

- `POST /atheneum/discoveries` — Store discoveries (symbols, CFG findings, patterns)
- `GET /atheneum/discoveries?target=X` — Query discoveries by symbol
- `POST /atheneum/handoffs` — Create handoff manifest
- `GET /atheneum/handoffs/pending` — Get pending handoffs
- `POST /atheneum/handoffs/{id}/claim` — Claim a handoff
- `GET /atheneum/knowledge?target=X` — Query atheneum knowledge graph
- All routes cfg-gated behind `atheneum` feature
- `ATHENEUM_DB` environment variable for atheneum database path

### MCP Tools

- `envoy_store_discovery` — Store discoveries to atheneum
- `envoy_query_knowledge` — Query atheneum knowledge
- `envoy_get_pending_handoff` — Get pending handoffs
- `envoy_claim_handoff` — Claim a handoff

### Grounded-Coding Skill Integration

- Gate 0: Query atheneum before local graph search
- Handoff protocol: HTTP-based (replaces `.grounded/handoff.md` files)
- Store discoveries: Symbol, CFG, issue, pattern findings

### CI/CD

- Fixed cfg-gating for atheneum feature in CI environment
- Atheneum dependency commented out for CI, feature set to `[]`
- All checks pass: fmt, clippy, test, e2e

## 0.2.0 — 2026-05-08

Hardening release — reliability, observability, and auditability.

### Async Safety (Phase 1)

- All `engine.lock().unwrap()` calls moved inside `tokio::task::spawn_blocking`
- Removed synchronous `with_graph` helper — all DB access is now non-blocking
- Prevents tokio worker thread starvation under concurrent load

### Input Validation & Caps (Phase 2)

- Body size cap: 1 MB max per text part
- Self-messaging rejection: `POST /messages` returns 422 if `from == to`
- Negative limit rejection: poll queries with `limit < 0` are clamped to 0
- Subscribe endpoint: verifies agent exists before creating subscription

### Graceful Shutdown (Phase 3)

- Signal handler for `SIGINT`/`SIGTERM`
- Drain in-flight requests before exiting
- Background tasks (nudge loop, CI monitor, doc monitor) get shutdown signal

### Circuit Breaker + TTL Eviction (Phase 4)

- `CircuitBreaker`: per-agent failure tracking with `Closed`/`Open`/`HalfOpen` states
- Configurable: `failure_threshold` (default 5), `cooldown_seconds` (default 60)
- `evict_stale()`: removes circuit entries for agents offline >24h
- `purge_offline()`: removes agents with no heartbeat >24h

### Async Process Commands (Phase 5)

- Replaced `std::process::Command` with `tokio::process::Command`
- All external process execution is now non-blocking

### Rate Limiting (C3)

- Per-IP rate limiting via `tower-governor`
- Default: 1000 req/s burst, 5000 req/s sustained
- `build_router_unlimited` for test environments

### Notification Delivery Tracking

- Failed WS pushes now store notifications as pending messages
- `MessageStore::store_notification()` for system-originated messages
- Offline agents receive stored notifications on poll/reconnect

### Audit Logging

- `AuditStore`: lightweight wrapper around `EventBus` for audit records
- Reserved project: `_envoy_audit`
- Operations tracked: `agent_registered`, `agent_disconnected`, `message_sent`,
  `event_ingested`, `circuit_opened`, `circuit_closed`
- `GET /audit?agent_id=&operation=&since=&limit=` for querying audit trail

### CI/CD

- GitHub Actions: 4 parallel jobs (fmt+clippy, tests, e2e, semgrep)
- Semgrep OSS: `p/rust` + custom rules (no-unwrap, no-todo)
- MSRV: Rust 1.95.0 (required for stable AVX512 in sqlitegraph)

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

- `ENVOY_DB` env var (default: `~/.local/share/envoy/server.db`)
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
