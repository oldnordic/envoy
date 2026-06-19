# Envoy Manual

## Installation

```bash
# From crates.io (recommended) — installs both `envoy` server and `envoy-hook`
cargo install agent-envoy

# From the grounded-coding installer (also installs magellan, llmgrep, mirage, splice)
curl -fsSL https://raw.githubusercontent.com/oldnordic/grounded-coding/master/install.sh | sh

# From source
git clone https://github.com/oldnordic/envoy.git
cd envoy
cargo build --release
# Binary at: target/release/envoy
```

## Running the Server

Envoy has two first-class transports. Neither is a "fallback" — they are for
different situations. Both run the exact same business logic; the only
difference is how a client reaches the server.

- **Unix domain socket** (`envoy local`) — for local development. One daemon
  process holds all the shared state (the agent registry, the session ledger,
  atheneum writes), and many local clients connect to it over a socket file.
  No TCP port, no firewall, no network config. This is the right choice when
  everything runs on one machine.
- **HTTP on a TCP port** (`envoy serve`) — for network access. Use this when a
  client is on another machine, or when a tool only speaks HTTP (any assistant
  in any language can reach it with plain `curl`, zero native client needed).

You pick the transport with a subcommand. Running `envoy` with no arguments
just prints usage — it does **not** start anything.

### `envoy local` — Unix domain socket (local dev)

Starts the daemon and listens on a Unix socket instead of a TCP port:

```bash
# Default socket: $XDG_RUNTIME_DIR/envoy.sock (e.g. /run/user/1000/envoy.sock)
envoy local

# Point at a custom socket and database
envoy local --socket /run/user/1000/envoy.sock --db ~/.local/share/envoy/server.db
```

Any local client can talk to it. With `curl`, use `--unix-socket`:

```bash
curl --unix-socket /run/user/1000/envoy.sock http://localhost/health
```

The socket is created with `0600` permissions (only your user can connect). A
leftover socket from a previous crash is cleaned up automatically on startup,
and the socket file is removed on graceful shutdown (Ctrl-C / SIGINT).

### `envoy serve` — HTTP on a TCP port (network)

Starts the HTTP server. This is what you want for network access or for tools
that only speak HTTP:

```bash
# Default: 127.0.0.1:9876, db at ~/.local/share/envoy/server.db
envoy serve

# Custom port and database
envoy serve --port 9876 --db /var/lib/envoy/server.db

# (equivalent, via environment variables)
ENVOY_PORT=9876 ENVOY_DB=/var/lib/envoy/server.db envoy serve
```

The server logs to stdout:
```
envoy serving HTTP on 127.0.0.1:9876, db=~/.local/share/envoy/server.db
```

### `envoy status` — introspection (binds nothing)

Reports whether a daemon is already running (on the socket and/or the port)
and what config it would use. It starts no server and binds nothing — safe to
run any time:

```bash
envoy status
```

### Backward compatibility

The old bare-flag form still works and is treated as `serve`. So
`envoy --port 9876` is exactly equivalent to `envoy serve --port 9876`. This
keeps existing scripts and systemd units working unchanged.

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ENVOY_DB` | `~/.local/share/envoy/server.db` | Path to the SQLite database. Created if it doesn't exist. |
| `ENVOY_PORT` | `9876` | TCP port for `envoy serve` (HTTP + WebSocket). |
| `XDG_RUNTIME_DIR` | `/run/user/$UID` | Directory holding the Unix socket used by `envoy local`. |

## Agent Lifecycle

### Registration

An agent registers by `POST`ing to `/agents` with a name, kind, and optional parent_id:

```bash
# Root agent (no parent)
curl -X POST http://127.0.0.1:9876/agents \
  -H "content-type: application/json" \
  -d '{"name":"claude","kind":"claude"}'

# Subagent (child of id1)
curl -X POST http://127.0.0.1:9876/agents \
  -H "content-type: application/json" \
  -d '{"name":"implement-task-3","kind":"claude","parent_id":"id1"}'
```

The server assigns IDs:
- Root agents: `id1`, `id2`, `id3`, ... (reuses retired IDs when available)
- Subagents: `id1.1`, `id1.2`, `id1.1.1`, ... (dot-notation based on parent)

Names are non-unique labels. IDs are the canonical identity.

### Idempotent Registration

Registering an agent with the same name twice returns the existing agent:

```bash
# First registration — creates new agent
curl -X POST http://127.0.0.1:9876/agents \
  -H "content-type: application/json" \
  -d '{"name":"hermessub1","kind":"worker"}'
# → {"agent_id":"id1","is_new":true,"name":"hermessub1",...}

# Second registration — returns existing agent (HTTP 200)
curl -X POST http://127.0.0.1:9876/agents \
  -H "content-type: application/json" \
  -d '{"name":"hermessub1","kind":"worker"}'
# → {"agent_id":"id1","is_new":false,"name":"hermessub1",...}
```

The response always includes:
- `agent_id` — use this in the `x-agent-id` header for all future requests
- `is_new` — `true` if created, `false` if returning existing
- `message` — explicit instruction with the assigned ID

### Retiring Agents

When an agent is retired, its numeric ID goes into a reuse pool:

```bash
# Retire agent id1
curl -X DELETE http://127.0.0.1:9876/agents/id1
# → {"disconnected":true,"affected":["id1"]}

# Register a new agent — reuses the retired ID
curl -X POST http://127.0.0.1:9876/agents \
  -H "content-type: application/json" \
  -d '{"name":"new_agent","kind":"worker"}'
# → {"agent_id":"id1","is_new":true,...}  (id1 reused!)
```

Only explicitly retired agents (via DELETE) have their IDs reused. Agents that
become offline due to server restart keep their IDs reserved.

### Server Restart Behavior

On restart, all agents from the database start as `Retired`. They must
re-register or send a heartbeat to become `Active` again.

### Heartbeats

Agents stay `Active` by sending periodic heartbeats. A heartbeat can be as
simple as "I'm alive" (no status), or carry a full or partial status snapshot.

```bash
# Lightweight heartbeat — agent is alive, status uses defaults
curl -X POST http://127.0.0.1:9876/heartbeat \
  -H "content-type: application/json" \
  -d '{"agent_id":"id1"}'

# Full status — every field provided
curl -X POST http://127.0.0.1:9876/heartbeat \
  -H "content-type: application/json" \
  -d '{
    "agent_id":"id1",
    "status":{
      "state":"blocked",
      "task_id":"task-42",
      "blocked_reason":"waiting on code review",
      "waiting_on_agent":"id2",
      "checkpoint":"implementation",
      "working_on":"refactoring auth module"
    }
  }'

# Partial status — only set the fields you care about; the rest use defaults
curl -X POST http://127.0.0.1:9876/heartbeat \
  -H "content-type: application/json" \
  -d '{
    "agent_id":"id1",
    "status":{"state":"working","working_on":"writing tests"}
  }'
```

Response (200):
```json
{
  "accepted": true,
  "nudges": []
}
```

Status fields (all optional — omit any and it falls back to the default):

| Field | Type | Default | Meaning |
|-------|------|---------|---------|
| `state` | `"working"` / `"blocked"` / `"waiting_review"` / `"idle"` | `working` | Current workflow state |
| `task_id` | string or null | null | Task the agent is working on |
| `blocked_reason` | string or null | null | Why the agent is blocked |
| `waiting_on_agent` | string or null | null | Agent this one is waiting on |
| `checkpoint` | string or null | null | Workflow checkpoint |
| `working_on` | string | `"heartbeat via WS"` | Human-readable description of current work |

If a heartbeat comes from an agent that is the **blocker** in an unresolved
dependency, the dependent agents are notified via WebSocket (`blocker_updated`
event) and the response includes a `nudges` array.

## Sending Messages

### Direct Message

```bash
curl -X POST http://127.0.0.1:9876/messages \
  -H "content-type: application/json" \
  -d '{
    "type": "direct",
    "from": "id1",
    "to": "id2",
    "parts": [
      {"text": "please review PR #42"}
    ]
  }'
```

Response (201):
```json
{
  "message_id": "1",
  "type": "direct",
  "from": "id1",
  "to": "id2",
  "task_id": null,
  "context_id": null,
  "timestamp": "2026-05-05T22:48:57.592+00:00",
  "sequence_id": 1,
  "parts": [
    {"text": "please review PR #42"}
  ]
}
```

### Handoff Message

A subagent handing work back to its parent:

```bash
curl -X POST http://127.0.0.1:9876/messages \
  -H "content-type: application/json" \
  -d '{
    "type": "handoff",
    "from": "id1.1",
    "to": "id1",
    "task_id": "task-003",
    "context_id": "ctx-001",
    "parts": [
      {"text": "context at 28%, handing off"},
      {"data": {
        "completion_status": "NEEDS_CONTEXT",
        "blocked_reason": null,
        "context_remaining_pct": 28,
        "what_was_done": [
          {"scope": "src/engine.rs", "change": "added publish()", "verified": true}
        ],
        "what_is_stubbed": [
          {"location": "src/http/", "reason": "context too low"}
        ],
        "remaining_work": ["Implement HTTP server"],
        "verification_state": {
          "tests_passing": 11,
          "tests_failing": 0,
          "quality_gate": {"passed": true, "blocking": 0, "warnings": 0},
          "cargo_check_passed": true
        },
        "magellan_trace": {
          "files_changed": ["src/engine.rs"],
          "symbols_added": ["fn publish"],
          "symbols_removed": [],
          "refs_in": {},
          "refs_out": {}
        },
        "grounded_queries_used": ["magellan find --name Engine"]
      }}
    ]
  }'
```

The handoff's `Data` part contains the structured `HandoffData` payload. The
`completion_status` field drives what the parent should do next:

| Status | Meaning |
|--------|---------|
| `DONE` | Work complete, ready for review |
| `DONE_WITH_CONCERNS` | Complete but has reservations — flagged for review |
| `BLOCKED` | Cannot proceed — `blocked_reason` is required |
| `NEEDS_CONTEXT` | Context window too low — parent should resume |



### Validation Rules

- At least one part is required
- Maximum 20 parts per message
- Text parts limited to 1 MB each
- `BLOCKED` status requires a `blocked_reason`
- `context_remaining_pct` must be 0–100

## Receiving Messages

### Polling (HTTP)

```bash
# Poll for agent id2, all messages since sequence 0
curl "http://127.0.0.1:9876/messages?to=id2&since=0&limit=10"

# Poll only new messages since sequence 5
curl "http://127.0.0.1:9876/messages?to=id2&since=5&limit=50"
```

Response:
```json
{
  "messages": [...],
  "latest_sequence": 7
}
```

The `since` parameter is a cursor: only messages with `sequence_id > since` are
returned. Use `latest_sequence` from the response as `since` in the next poll.

Limit is capped at 100.

### WebSocket (Real-Time Push)

Connect to the WebSocket endpoint for instant delivery:

```javascript
const ws = new WebSocket("ws://127.0.0.1:9876/ws/id2");

ws.onmessage = (event) => {
  const { event: type, data } = JSON.parse(event.data);
  switch (type) {
    case "agent_connected":
      console.log("Connected as", data.agent_id);
      break;
    case "message":
      console.log("New message from", data.from, ":", data.parts);
      break;
  }
};
```

On connect, envoy sends:
1. **Catch-up**: all undelivered messages for that agent (as individual `message` events)
2. **`agent_connected`**: confirmation the agent is online and receiving

After that, new messages sent via `POST /messages` where `to` matches your agent_id
are pushed in real time.

The client can send text frames as heartbeats — they're acknowledged but ignored by
the server. The server never initiates a close unless the agent is offline.

## Dependencies

Agents can declare that they are blocked by another agent. When the blocker
sends a heartbeat, the dependent agent is notified (WebSocket `blocker_updated`
event).

### Create a Dependency

```bash
curl -X POST http://127.0.0.1:9876/dependencies \
  -H "content-type: application/json" \
  -d '{
    "dependent_agent":"id1",
    "blocker_agent":"id2",
    "reason":"waiting on id2 to finish the API refactor"
  }'
```

Response (201):
```json
{
  "dependency_id": "5",
  "dependent_agent": "id1",
  "blocker_agent": "id2",
  "reason": "waiting on id2 to finish the API refactor",
  "created_at": "2026-06-19T14:00:00Z",
  "resolved": false
}
```

- **`dependent_agent`** — the agent that is *blocked* (waiting).
- **`blocker_agent`** — the agent that is *doing the blocking* (must finish first).

### Querying Dependencies

Two complementary views are available. Use the unambiguous aliases to avoid
confusion:

```bash
# What is id2 blocking? (id2 is the blocker — others are waiting on it)
curl http://127.0.0.1:9876/dependencies/blocking/id2

# What is id1 blocked by? (id1 is the dependent — it is waiting on others)
curl http://127.0.0.1:9876/dependencies/blocked-by/id1
```

The original route names are kept for backward compatibility:

| Alias (preferred) | Original | Returns |
|-------------------|----------|---------|
| `/dependencies/blocking/{id}` | `/dependencies/blocker/{id}` | Unresolved deps where `{id}` **is the blocker** |
| `/dependencies/blocked-by/{id}` | `/dependencies/dependent/{id}` | Unresolved deps where `{id}` **is the dependent** |

Only unresolved (not yet resolved) dependencies are returned.

### Resolving a Dependency

```bash
curl -X POST http://127.0.0.1:9876/dependencies/5/resolve
```

The dependent agent is notified via WebSocket that the dependency is resolved.

## Monitoring

### Health Check

```bash
curl http://127.0.0.1:9876/health
```

```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "agents_online": 3
}
```

Every response also includes an `x-request-id` header (a UUID) for log correlation. This lets you trace a request through envoy's logs by its unique ID.

### Stats

```bash
curl http://127.0.0.1:9876/stats
```

```json
{
  "messages_total": 42,
  "agents_registered": 5
}
```

### Prometheus Metrics

Envoy exposes a `/metrics` endpoint in Prometheus exposition format. No authentication required -- it's a public endpoint, just like `/health`.

```bash
curl http://127.0.0.1:9876/metrics
```

Example output:

```
# HELP envoy_requests_total Total HTTP requests processed, labeled by operation and status class
# TYPE envoy_requests_total counter
envoy_requests_total{method="GET",path="/health",status="2xx"} 14
envoy_requests_total{method="POST",path="/messages",status="2xx"} 7

# HELP envoy_agents_online Number of currently active agents
# TYPE envoy_agents_online gauge
envoy_agents_online 3

# HELP envoy_request_duration_ms Request latency in milliseconds, labeled by operation
# TYPE envoy_request_duration_ms histogram
envoy_request_duration_ms_bucket{path="/health",le="0.5"} 14
envoy_request_duration_ms_sum{path="/health"} 0.821
envoy_request_duration_ms_count{path="/health"} 14
```

**What the metrics mean:**

| Metric | Type | Labels | What it tells you |
|--------|------|--------|-------------------|
| `envoy_requests_total` | counter | `method`, `path`, `status` | How many HTTP requests, broken down by method (GET/POST/DELETE), normalized path, and status class (2xx/4xx/5xx) |
| `envoy_request_duration_ms` | histogram | `path` | Request latency. Buckets: 0.5ms, 1ms, 5ms, 10ms, 50ms, 100ms, 500ms, 1s, 5s |
| `envoy_agents_online` | gauge | (none) | Number of currently active agents |

**Path normalization:** URL segments that look like IDs (numeric like `42`, named like `id1121`, or UUIDs like `338b8adc-6c08-4664-af1d-69300e7c576a`) are collapsed to `:id`. This prevents cardinality explosion -- you get `/agents/:id/messages` instead of a different label for every agent.

**Prometheus scrape config:**

```yaml
scrape_configs:
  - job_name: 'envoy'
    static_configs:
      - targets: ['127.0.0.1:9876']
    scrape_interval: 15s
    metrics_path: /metrics
```

### Request Tracing

Every HTTP response includes an `x-request-id` header with a unique UUID. To see trace-level logs for requests, set:

```bash
RUST_LOG=tower_http=debug envoy
```

This logs each request's method, path, status code, and latency -- tagged with the request ID so you can correlate logs to specific requests.

## Database

Envoy stores all messages in a single SQLite database. The schema:

```sql
CREATE TABLE envoy_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    msg_type TEXT NOT NULL,
    from_agent TEXT NOT NULL,
    to_agent TEXT NOT NULL,
    task_id TEXT,
    context_id TEXT,
    timestamp TEXT NOT NULL,
    sequence_id INTEGER NOT NULL,
    parts_json TEXT NOT NULL
);

CREATE INDEX idx_envoy_messages_to_seq
    ON envoy_messages(to_agent, sequence_id);
```

The database can be inspected directly:

```bash
sqlite3 ~/.envoy/server.db "SELECT id, msg_type, from_agent, to_agent, sequence_id FROM envoy_messages;"
```

## Error Handling

All errors return JSON with `code` and `message`:

```json
{
  "error": {
    "code": "AGENT_OFFLINE",
    "message": "agent offline: id1"
  }
}
```

| HTTP Status | Error Code | Condition |
|-------------|------------|-----------|
| 404 | `AGENT_NOT_FOUND` | Agent doesn't exist |
| 409 | `AGENT_OFFLINE` | Agent is disconnected |
| 409 | `AGENT_ALREADY_EXISTS` | Duplicate registration |
| 404 | `MESSAGE_NOT_FOUND` | Message ID doesn't exist |
| 404 | `CHANNEL_NOT_FOUND` | Channel doesn't exist |
| 400 | `INVALID_MESSAGE` | Validation failed |
| 400 | `MESSAGE_TOO_LARGE` | Text part exceeds 1 MB |
| 400 | `TOO_MANY_PARTS` | More than 20 parts |
| 400 | `SERIALIZATION_ERROR` | Invalid JSON body |
| 500 | `INTERNAL_ERROR` | Database or graph error |

## License

GPL-3.0-only
