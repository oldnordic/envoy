# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-07-31

### Added

- **Central knowledge store in cross-project search** (`src/atheneum_bridge/cross.rs`):
  `GET /atheneum/cross/search` and `GET /atheneum/cross/navigate` now chain
  `with_central_knowledge_db` onto the `CrossRouter`, so results include the
  central atheneum store (surfaced as the synthetic `__atheneum_central__`
  project) alongside per-project code indexes.
- **envoy-mcp retired-agent filter** (`envoy-mcp/backend.rs`): `list_agents`
  filters `lifecycle == "retired"` agents by default and reports
  `total` / `total_unfiltered` / `retired_filtered` counts, keeping operator
  views readable as the daemon accumulates registrations. Agents without a
  lifecycle field are preserved. Salvaged from an uncommitted contributor
  worktree; includes unit tests.

### envoy-mcp — orchestration composite tools

- **delegate_task** (`envoy-mcp/backend.rs`, `envoy-mcp/tools.rs`): creates a
  persistent handoff with goal + context and returns immediately. The
  receiving agent picks it up when ready. Survives agent restarts. One MCP
  call replaces store_handoff with a structured manifest.
- **declare_workflow** (`envoy-mcp/backend.rs`, `envoy-mcp/tools.rs`): declares
  a multi-agent dependency graph in one call. Each edge says blocker must
  finish before dependent starts. Envoy enforces ordering across restarts.
- **status_snapshot** (`envoy-mcp/backend.rs`, `envoy-mcp/tools.rs`): dashboard
  view — daemon health + all agents + pending handoffs for the caller. One
  MCP call replaces three round trips.
- envoy-mcp tool count: 15 → 18.

### Changed

- **`atheneum` dependency 0.8 → 0.12.2 from crates.io** (`Cargo.toml`): the
  path dependency on a local checkout is gone; envoy now tracks the published
  crate. Pulls in the CrossRouter central-store fix and the unified tool API
  groundwork. This also let CI drop its "comment out the private dependency"
  sed workaround — the atheneum feature is now compiled and tested on CI.
- **crossbeam-epoch 0.9.18 → 0.9.20** (`Cargo.lock`): fixes
  RUSTSEC-2026-0204 (invalid pointer dereference in `fmt::Pointer`), pulled
  in transitively via rayon/sqlitegraph.

### Fixed

- **Test suite**: `setup_test_state_without_atheneum` dropped its `TempDir`
  on return, deleting the database directory under the open engine; SQLite
  WAL creation then failed with "disk I/O error". The helper now returns the
  `TempDir` alongside the state.
- **Clippy** (`src/bin/hook.rs`): redundant field name in struct init and
  `filter(..).last()` on a double-ended iterator (now `rfind`), surfaced by
  the `-D warnings` gate once the atheneum feature compiled on CI.

### Security

- **cargo-audit**: `RUSTSEC-2020-0021` (rio, unmaintained, UAF on leaked
  future) accepted with justification in `.cargo/audit.toml` — rio reaches
  envoy only through sqlitegraph's experimental io_uring coordinator; an
  atheneum task tracks removing it upstream.

## [0.3.1] - 2026-06-22

### Added

- **HTTP read-only observability endpoints** (`GET /atheneum/sessions/{id}`, `GET /atheneum/tool-calls/recent`, `GET /atheneum/handoffs/recent`):
  - `GET /atheneum/sessions/{id}` returns the session summary along with its associated events and tool calls, allowing direct tracing.
  - `GET /atheneum/tool-calls/recent` allows querying recent tool calls with optional `session_id` filter and aggregates tool usage counts.
  - `GET /atheneum/handoffs/recent` retrieves recent handoffs with optional `project` and `agent` filtering.
- Fully wired the new endpoints to the routing framework and the integration test harness.

### Changed

- **Mandatory Agent Identity Enforcement**: Verified and aligned agent identity parameters (`agent_id`, `agent_name`) across write-path payloads to guarantee provenance.
- **`project_name` resolves the git toplevel basename** (`src/bin/hook.rs`): `cmd_session_start` / `cmd_tool_call` / `cmd_session_end` / `cmd_subagent_end` now tag sessions with the repository name (via `git rev-parse --show-toplevel`) instead of the cwd's immediate parent. Fixes worktree/subdir launches being tagged `tmp` or with a subdirectory name. Falls back to the dir basename outside a git worktree. Covered by `project_name_uses_git_toplevel_basename`.
- **`atheneum` dependency bumped to `^0.8`** (`Cargo.toml`): tracks the atheneum 0.8.0 release (session-digest composer, `thread` decision-chain navigation, `semantic-search` now opt-in). The bridge code is unchanged; this is a version-constraint update only.

## [0.3.0] - 2026-06-19

### Added

- **Subcommand dispatch and two first-class transports** (`main.rs`, `server.rs`): envoy no longer binds a TCP port on every invocation. It now has explicit subcommands, so starting a server is one mode among several rather than the only thing it does:
  - `envoy local` — run as a local daemon over a **Unix domain socket** (no TCP port). This is the primary local-dev transport: one daemon, shared state, many clients, no port allocation, no firewall. Default socket path is `$XDG_RUNTIME_DIR/envoy.sock` (i.e. `/run/user/$UID/envoy.sock`). It serves the *identical* router as HTTP, via axum's native `UnixListener` support — transport is ingress only, all business logic is shared.
  - `envoy serve` — run the HTTP server on a TCP port (`--port`, default `$ENVOY_PORT` or 9876). This is the network access + universal curl transport, now explicit opt-in.
  - `envoy status` — introspection: reports daemon liveness and config. Binds nothing, starts no server.
- **Unix domain socket transport** (`server.rs::run_local`): serves the full HTTP API over a Unix socket with 0600 permissions; stale sockets are unlinked on startup and on graceful (SIGINT/SIGTERM) shutdown. Contactable by any local client via `curl --unix-socket`, `nc -U`, or a native Rust/Python Unix-socket client.

### Changed

- **Bare-flag invocation is backward-compatible.** `envoy --port 9876` (the old default-everything form) is treated as `envoy serve --port 9876`. Running `envoy` with no arguments now prints usage instead of binding the port.
- **Bumped `atheneum` dependency** `^0.5.0` → `^0.7` (stale version constraint that no longer resolved against the local atheneum 0.7.x).

### Fixed

- **No more unconditional port bind.** Previously *any* envoy invocation — including `--help` and `status` checks — started the HTTP listener on `:9876`. With subcommand dispatch the port only binds when you explicitly ask for `serve`.
- **`envoy <subcommand> --help` no longer hangs.** The custom arg parser only intercepted `--help`/`--version` when they appeared as the first argument (`args[1]`). After a subcommand (`envoy local --help`, `envoy serve --version`), the flag was silently swallowed by `parse_flags` and the daemon bound its socket/port forever. The parser now scans all post-subcommand args for `-h`/`--help`/`-V`/`--version` and prints-and-exits before any server starts (`main.rs`).
- **Heartbeat accepts lightweight and partial status.** `POST /heartbeat` required a fully-populated `status` object — sending `{"agent_id":"id1"}` (a lightweight "I'm alive" ping) or a partial snapshot (only `state`) was rejected with HTTP 422. `HeartbeatRequest.status` and `AgentStatusSnapshot` are now `#[serde(default)]`, so a missing status defaults to the snapshot (consistent with the WebSocket heartbeat handler) and omitted fields fall back to their defaults (`status.rs`). This makes the HTTP heartbeat endpoint usable for both a trivial keep-alive and a full status update.
- **Unambiguous dependency route aliases.** `/dependencies/blocker/{id}` returns deps where `{id}` *is the blocker*, and `/dependencies/dependent/{id}` returns deps where `{id}` *is the dependent* — correct but easy to query the wrong direction from the names alone. Added two unambiguous aliases: `/dependencies/blocking/{id}` ("what is `{id}` blocking?") and `/dependencies/blocked-by/{id}` ("what is `{id}` blocked by?"). Both pairs return identical results; the originals are kept for backward compatibility (`router.rs`). `MANUAL.md` gained a Dependencies section documenting create/query/resolve with the direction semantics spelled out.

## [0.2.0] - 2026-06-12

### Added

- **Prometheus `/metrics` endpoint** — New public endpoint exposing request counters (`envoy_requests_total` by method/path/status), latency histograms (`envoy_request_duration_ms` with configurable buckets), and gauges (`envoy_agents_online`). Uses the `metrics` facade with `metrics-exporter-prometheus`. Path normalization collapses numeric, `id*`, and UUID segments into `:id` to prevent cardinality explosion. Agent lifecycle handlers (register, disconnect, retire) update the `agents_online` gauge automatically. Endpoint bypasses auth (public, like `/health`).

### Changed

- **`parking_lot::Mutex` everywhere** — Replaced all `std::sync::Mutex` with `parking_lot::Mutex` across 7 files: `state.rs`, `agent.rs`, `circuit.rs`, `monitor/ci.rs`, `monitor/doc.rs`, `server.rs`, `error.rs`. Benefits: no poisoning (removed `LockPoisoned` error variant and `recover_lock` helper), smaller footprint (~1 byte vs ~40 bytes per lock), faster uncontended path. All `.lock().unwrap_or_else(|e| e.into_inner())` and `.lock().map_err(|e| EnvoyError::LockPoisoned(...))` patterns simplified to `.lock()`.
- **tower-http features expanded** — Added `request-id`, `trace`, `timeout`, `util` features to `tower-http` dependency. Router now layers `SetRequestIdLayer`, `PropagateRequestIdLayer`, and `TraceLayer` onto every request. Every response includes an `x-request-id` header (UUID) for log correlation. Request tracing is active when `RUST_LOG=tower_http=debug` is set.

### Fixed

- **`cross_navigate` graph query column mismatch** — The BFS edge query used `kind` but production magellan databases use `edge_type`. Changed to `SELECT id, edge_type AS kind, ...` so the alias works with both schemas. The `cross/search` endpoint was unaffected (queries `graph_entities` which does use `kind`). Test fixture updated to match production schema.
- **All evidence POST handlers now return JSON** — 8 handlers (`post_prompt`, `post_tool_call`, `post_file_write`, `post_commit`, `post_test_run`, `post_fix_chain`, `post_bench_run`, `post_subagent_handover`) previously returned bare `201 Created` with no body. Now all return `{"recorded": true}` so callers can confirm success without relying on HTTP status alone.
- **API.md rewritten from source** — Every endpoint now documented with correct request fields (verified against struct definitions), required/optional markers, and response shapes. Notes the MCP polling limitation for messaging.

### Added

#### Cross-Project Query Endpoints — Search All Your Codebases From One Place

Envoy now exposes HTTP endpoints that let you search and navigate across multiple magellan-indexed projects without copying data. This solves a real problem: when you work on `envoy`, `magellan`, and `atheneum` simultaneously, you often need to know "which project has the `build_router` function?" or "how does each project handle error recovery?" Previously you had to run `magellan` queries per project. Now you query once through envoy.

**New endpoints:**

| Endpoint | What it does |
|----------|-------------|
| `GET /atheneum/cross/search?q=...&language=...&k=N` | Search symbols across all registered projects |
| `GET /atheneum/cross/navigate?q=...&language=...&k=N&depth=D` | Search + BFS subgraph walk per project |

**Example:**

```bash
# Register projects in atheneum (one-time setup)
atheneum meta-register envoy ~/Projects/envoy ~/.magellan/envoy/envoy.db --language rust

# Search for "build_router" across all Rust projects
curl "http://127.0.0.1:9876/atheneum/cross/search?q=build_router&language=rust&k=10"

# Navigate: find "build_router" and expand 2 hops of graph context
curl "http://127.0.0.1:9876/atheneum/cross/navigate?q=build_router&language=rust&k=3&depth=2"
```

**How it works:**
1. Envoy delegates to atheneum's `CrossRouter`, which reads the `meta.db` routing registry.
2. For each registered project, the router lazily `ATTACH DATABASE` the project's magellan DB (read-only).
3. Queries run as cross-schema `UNION ALL` over `graph_entities` and `graph_edges`.
4. An LRU cache (default capacity 8) keeps hot DBs attached across requests. Missing or unreadable DBs are skipped with a warning.
5. Language filtering limits the search to projects tagged with that language at registration time.

**Limit:** SQLite defaults to 10 max attached databases. The cache defaults to 8 to stay safely under that limit.

#### Graph Navigation Endpoints

New `GET /atheneum/graph/*` routes for reading the knowledge graph directly:

| Endpoint | Purpose |
|----------|---------|
| `GET /atheneum/graph/entities/{id}` | Read entity by ID |
| `GET /atheneum/graph/edges/{id}` | Read edge by ID |
| `GET /atheneum/graph/entities/{id}/neighbors?depth=N` | One-hop edges (depth=0) or BFS subgraph (depth>0) |
| `GET /atheneum/graph/navigate?query=X[&k=N&depth=D&project=P]` | Semantic search + graph walk |
| `GET /atheneum/graph/stats` | Topological summary (entity + edge counts by kind/type) |

#### Generic Event Logging

- **`POST /atheneum/events`** — Log arbitrary events with `session_id`, `event_type`, `entity_id`, and JSON `payload`. Persists to the `event_log` table for cross-session auditing.
- **`GET /atheneum/events`** — Query events with filters for `session_id`, `event_type`, and `limit`.

#### Typed Hook Provenance from `envoy-hook`

The `envoy-hook` binary now emits first-class Atheneum evidence instead of just coarse tool-call summaries:

- File reads and path inspection tools → `accessed` relations via `POST /atheneum/events`
- Successful `Write` / `Edit` calls → `modified` relations via `POST /atheneum/events`
- Test commands (`cargo test`, `pytest`, etc.) → `POST /atheneum/test-runs`
- Tool errors → failure relation events + discovery records

#### Semantic Search Auto-Index

`GET /atheneum/search` no longer rebuilds the HNSW index on every request. Discoveries are auto-indexed on write in `store_discovery()`.

#### Cross-Project Sessions

`GET /atheneum/sessions` now supports cross-project queries. The `project` parameter is optional. When omitted, returns sessions from all projects.

### Fixed

- **CI and doc monitors now env-gated** — `ENVOY_CI_MONITOR` and `ENVOY_DOC_MONITOR` are off by default. Previously they hardcoded polling `oldnordic/magellan` CI and watching `.` on every startup.
- **Default DB path uses `$XDG_DATA_HOME`/`$HOME`** — no longer hardcoded to `/home/feanor/.envoy/server.db`.
- **`build_router_unlimited()` includes atheneum routes** — test helper now correctly mounts bridge routes when the `atheneum` feature is active. Fixes 3 failing integration tests.
- **Removed `dashboard` feature** — `dashboard.rs` (498 LOC) was never compiled or routed. Feature flag, source file, and 3 test files deleted.
- **SQL parameter ordering bug in `query_sessions`** — Fixed a mismatch where `parent_id` parameters were incorrectly placed when `project` was `None` but `parent_id` was `Some`, causing runtime SQLite parameter count errors.
- **CI workflow dependency stripping** — `sed` command now correctly comments out both `[dependencies]` and `[dev-dependencies]` `atheneum` path entries.
- **Atheneum v0.5.0 API compatibility** — All bridge handlers updated for breaking changes: `query_knowledge_in_project` gained `max_tokens`, `lexical_search` gained `entity_kind`/`max_tokens`, `navigate` gained `entity_kind`/`max_tokens`, `PromptParams`/`ToolCallParams`/`FileWriteParams` gained new fields.

## [0.1.1] — 2026-06-09

### Fixed

- **Atheneum v0.5.0 API compatibility** — Updated all bridge handlers for atheneum 0.5.0 API surface.

### Performance

- **Atheneum graph connection pooling** — `AppState::with_atheneum_async()` caches `AtheneumGraph` in `Arc<Mutex<Option<AtheneumGraph>>>` and reuses it across `spawn_blocking` tasks. Eliminates per-request `AtheneumGraph::open()` (~50 call sites replaced). Before: every HTTP request opened a new SQLite connection (~50–100ms). After: graph opens once on first request, then reused indefinitely.
- **`parking_lot::Mutex` for atheneum graph cache** — Switched from `std::sync::Mutex` to `parking_lot::Mutex`. Benefits: no poisoning, ~1 byte vs ~40 bytes, faster uncontended path.

### Agent Registry (Breaking)

- **Idempotent registration** — `POST /agents` with the same name returns the existing agent with `is_new: false` and HTTP 200 instead of creating duplicates.
- **Retired ID reuse pool** — Explicitly retired agent IDs (via `DELETE /agents/{id}`) are added to a reuse pool. New registrations reuse the lowest available retired ID.
- **Server restart lifecycle** — On restart, all agents loaded from the database start as `Retired`. They must re-register or send a heartbeat to become `Active` again.
- **Server-assigned identification** — Registration response now includes `agent_id`, `is_new`, and an explicit usage instruction.

### Architecture

- **Split `src/http.rs` (1968 LOC) into focused modules** — `src/http/` directory with `state.rs`, `middleware.rs`, `router.rs`, `types.rs`, `handlers/` (8 modules), and `ws.rs`.
- **Split `src/atheneum_bridge.rs` (~2400 LOC) into focused modules** — `src/atheneum_bridge/` directory with `utils.rs`, `types.rs`, `discovery.rs`, `tasks.rs`, `actions.rs`, `ontology.rs`, `import.rs`, `sessions.rs`, and `cross.rs`.

## 0.3.0 — 2026-05-09

### Atheneum Integration — Cross-Agent Knowledge Sharing

- `POST /atheneum/discoveries` — Store discoveries (symbols, CFG findings, patterns)
- `GET /atheneum/discoveries?target=X` — Query discoveries by symbol
- `POST /atheneum/handoffs` — Create handoff manifest
- `GET /atheneum/handoffs/pending` — Get pending handoffs
- `POST /atheneum/handoffs/{id}/claim` — Claim a handoff
- `GET /atheneum/knowledge?target=X` — Query knowledge graph
- All routes cfg-gated behind `atheneum` feature
- `ATHENEUM_DB` environment variable for atheneum database path

### MCP Tools

- `envoy_store_discovery`, `envoy_query_knowledge`, `envoy_get_pending_handoff`, `envoy_claim_handoff`

## 0.2.0 — 2026-05-08

### Reliability and Observability

- All `engine.lock().unwrap()` calls moved inside `tokio::task::spawn_blocking`
- Body size cap: 1 MB max per text part
- Self-messaging rejection, negative limit clamping, subscription verification
- Graceful shutdown with `SIGINT`/`SIGTERM` handling
- Circuit breaker with `Closed`/`Open`/`HalfOpen` states
- `tokio::process::Command` for all external process execution
- Per-IP rate limiting via `tower-governor`
- Failed WS pushes store notifications as pending messages
- `AuditStore` with `_envoy_audit` reserved project
- GitHub Actions CI: fmt+clippy, tests, e2e, semgrep

## 0.1.0 — 2026-05-05

### Initial MVP

- Agent registry with hierarchical IDs (`id1`, `id1.1`, etc.)
- Message store with SQLite persistence and cursor-based polling
- WebSocket push with broadcast channels
- HTTP server with axum: agents CRUD, messages send/poll/get, health, stats
- Handoff workflow with end-to-end test
- 19 total tests passing

### fix(envoy-mcp): ack_message 415 + 422 errors

Two bugs in envoy_ack_message:

1. **415 Unsupported Media Type**: post_empty sent POST with no body and no
   Content-Type. Daemon's axum handler expects application/json. Fixed:
   post_empty now sends `Content-Type: application/json` with empty `{}` body.

2. **422 missing agent_id**: daemon's AckRequest struct requires
   `{"agent_id": "..."}`. The MCP was sending `{}`. Fixed: ack_message now
   reads the backend's self.agent_id and sends it in the JSON body.

### fix(envoy-mcp): pending_handoff race (documented, not a code bug)

pending_handoff returns null when store_handoff and pending_handoff are
pipelined together (rmcp dispatches concurrently, pending can run before
store completes). With sequential timing (real MCP client behavior), the
handoff is found correctly. Not a code bug — same rmcp concurrency
artifact documented in the atheneum-mcp memory tests.
