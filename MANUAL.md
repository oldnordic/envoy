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

```bash
# Default: 127.0.0.1:9876, db at ~/.envoy/server.db
envoy

# Custom port and database
ENVOY_PORT=9876 ENVOY_DB=/var/lib/envoy/server.db envoy
```

The server logs to stdout:
```
envoy server listening on 127.0.0.1:9876, db=~/.local/share/envoy/server.db
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ENVOY_DB` | `~/.local/share/envoy/server.db` | Path to the SQLite database. Created if it doesn't exist. |
| `ENVOY_PORT` | `9876` | TCP port for HTTP + WebSocket. |

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
