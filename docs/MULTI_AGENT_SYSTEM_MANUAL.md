# Multi-Agent System Manual

**Version:** 1.0.0  
**Last Updated:** 2026-05-10  
**Components:** Envoy, Atheneum, Hermes, Magellan

---

## System Overview

This is a local-first multi-agent coordination system for AI coding agents. It enables:

- **Message passing** between agents (direct, broadcast, handoff)
- **Knowledge persistence** via Atheneum (sqlitegraph-based vector store)
- **Code intelligence** via Magellan (symbol/reference/CFG queries)
- **Coordination bus** via Envoy (SQLite-based pub/sub)

All components run locally, require no cloud services, and store data in SQLite databases.

---

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Agent A   │────▶│    Envoy    │◀────│   Agent B   │
│  (Claude)   │     │   (HTTP)    │     │  (Claude)   │
└─────────────┘     └──────┬──────┘     └─────────────┘
                           │
                    ┌──────▼──────┐
                    │  Atheneum   │
                    │  (sqlitegraph)  │
                    └─────────────┘
```

**Component responsibilities:**

| Component | Purpose | Data Location |
|-----------|---------|---------------|
| **Envoy** | Message broker, agent registry | `~/.envoy/server.db` |
| **Atheneum** | Vector store, semantic search | `~/.envoy/atheneum.db` |
| **Magellan** | Code graph, symbols, CFG | `.magellan/<project>.db` |
| **Hermes** | Knowledge layer, coordination | (in development) |

---

## Installation

### 1. Build Envoy

```bash
cd /path/to/envoy
cargo build --release
```

Binary: `target/release/envoy`

### 2. Initialize Atheneum

```bash
cd /path/to/atheneum
cargo build --release
./target/release/atheneum init ~/.envoy/atheneum.db
```

Expected output:
```
Initializing Atheneum graph at: ~/.envoy/atheneum.db
✅ Graph initialized successfully
   Health: OK
```

### 3. Install Envoy Plugin

The envoy plugin is already installed at:
```
~/.claude/plugins/envoy/
```

Plugin configuration: `~/.claude/plugins/envoy/.claude-plugin/plugin.json`

---

## Running the System

### Start Envoy Server

```bash
# With Atheneum integration (recommended)
ATHENEUM_DB=~/.envoy/atheneum.db envoy

# Custom port/database
ENVOY_PORT=9876 ENVOY_DB=~/.envoy/server.db ATHENEUM_DB=~/.envoy/atheneum.db envoy
```

The server logs to stdout:
```
envoy server listening on 127.0.0.1:9876, db=~/.envoy/server.db
atheneum integration enabled: ~/.envoy/atheneum.db
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ENVOY_DB` | `~/.envoy/server.db` | Path to envoy SQLite database |
| `ENVOY_PORT` | `9876` | HTTP + WebSocket port |
| `ATHENEUM_DB` | (none) | Path to Atheneum database (optional) |

### Verify Health

```bash
curl http://localhost:9876/health
```

Response:
```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "agents_online": 3,
  "atheneum_configured": true
}
```

---

## Agent Lifecycle

### Registration

Agents register via `POST /agents`:

```bash
# Root agent (no parent)
curl -X POST http://localhost:9876/agents \
  -H "content-type: application/json" \
  -d '{"name":"claude","kind":"claude"}'

# Subagent (child of id1)
curl -X POST http://localhost:9876/agents \
  -H "content-type: application/json" \
  -d '{"name":"implement-task-3","kind":"claude","parent_id":"id1"}'
```

**ID assignment:**
- Root agents: `id1`, `id2`, `id3`, ...
- Subagents: `id1.1`, `id1.2`, `id1.1.1`, ... (dot notation)

### Disconnect

```bash
curl -X DELETE http://localhost:9876/agents/id1
# → {"disconnected":true,"affected":["id1","id1.1"]}
```

Undelivered messages are preserved in the tombstone endpoint:
```bash
curl http://localhost:9876/agents/id1/messages/pending
```

---

## Message Types

### 1. Direct Message

Point-to-point communication:

```bash
curl -X POST http://localhost:9876/messages \
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

### 2. Handoff Message

Subagent returning work to parent with context:

```bash
curl -X POST http://localhost:9876/messages \
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
        "context_remaining_pct": 28,
        "what_was_done": [
          {"scope": "src/engine.rs", "change": "added publish()", "verified": true}
        ],
        "remaining_work": ["Implement HTTP server"],
        "verification_state": {
          "tests_passing": 11,
          "tests_failing": 0,
          "cargo_check_passed": true
        }
      }}
    ]
  }'
```

**Completion statuses:**
| Status | Meaning |
|--------|---------|
| `DONE` | Work complete, ready for review |
| `DONE_WITH_CONCERNS` | Complete but flagged for review |
| `BLOCKED` | Cannot proceed |
| `NEEDS_CONTEXT` | Context window too low |

### 3. Broadcast Message

Send to all agents:

```bash
curl -X POST http://localhost:9876/messages \
  -H "content-type: application/json" \
  -d '{
    "type": "broadcast",
    "from": "id1",
    "parts": [
      {"text": "system shutdown in 5 minutes"}
    ]
  }'
```

---

## Receiving Messages

### HTTP Polling

```bash
# Poll for agent id2, all messages since sequence 0
curl "http://localhost:9876/messages?to=id2&since=0&limit=10"

# Poll only new messages since sequence 5
curl "http://localhost:9876/messages?to=id2&since=5&limit=50"
```

Response:
```json
{
  "messages": [...],
  "latest_sequence": 7
}
```

### WebSocket (Real-Time)

```javascript
const ws = new WebSocket("ws://localhost:9876/ws/id2");

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

---

## Atheneum Integration

When `ATHENEUM_DB` is set, Envoy persists messages to Atheneum for semantic search:

```bash
# Search for messages about "rust"
curl "http://localhost:9876/atheneum/search?q=rust&limit=10"
```

Atheneum uses sqlitegraph's vector store for similarity search over message content.

---

## Monitoring

### Stats

```bash
curl http://localhost:9876/stats
```

```json
{
  "messages_total": 42,
  "agents_registered": 5
}
```

### List Agents

```bash
curl http://localhost:9876/agents
```

```json
{
  "agents": [
    {"agent_id": "id1", "name": "claude", "kind": "claude", "online": true},
    {"agent_id": "id1.1", "name": "subagent-1", "kind": "worker", "online": false}
  ]
}
```

---

## Claude Code Plugin

The envoy plugin provides MCP tools for Claude Code:

**Installation:** `~/.claude/plugins/envoy/`

**Tools:**
- `envoy_send` — Send messages to other agents
- `envoy_receive` — Poll for new messages
- `envoy_status` — Check agent status

**Monitor:** Background script polls for messages when WebSocket unavailable

---

## Troubleshooting

### Port already in use

```bash
lsof -i :9876
kill <PID>
```

### Database locked

```bash
# Check for stale connections
sqlite3 ~/.envoy/server.db "SELECT * FROM sqlite_master;"
```

### Atheneum not configured

```bash
# Re-initialize Atheneum
cd /path/to/atheneum
./target/release/atheneum init ~/.envoy/atheneum.db

# Restart Envoy with ATHENEUM_DB
pkill envoy
ATHENEUM_DB=~/.envoy/atheneum.db envoy
```

### Messages not delivered

1. Check agent is online: `curl http://localhost:9876/agents`
2. Check message tombstone: `curl http://localhost:9876/agents/<id>/messages/pending`
3. Verify sequence cursor is correct

---

## Development

### Project Locations

| Project | Path |
|---------|------|
| Envoy | `/path/to/envoy` |
| Atheneum | `/path/to/atheneum` |
| Magellan | `/path/to/project |
| Hermes | `/path/to/project |

### Building from Source

```bash
# Envoy
cd /path/to/envoy
cargo build --release

# Atheneum
cd /path/to/atheneum
cargo build --release
```

### Running Tests

```bash
# Envoy
cd /path/to/envoy
cargo test

# Atheneum
cd /path/to/atheneum
cargo test
```

---

## License

GPL-3.0

---

## References

- [Envoy API Documentation](/path/to/envoy/API.md)
- [Envoy Manual](/path/to/envoy/MANUAL.md)
- [Atheneum Documentation](/path/to/atheneum/)
- [Magellan Documentation](/path/to/project)
