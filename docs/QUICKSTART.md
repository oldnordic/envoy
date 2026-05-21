# Multi-Agent System Quick Start

**5 minutes to running multi-agent coordination**

---

## Prerequisites

- Rust toolchain (1.70+)
- Node.js 18+ (for MCP plugin)
- Claude Code with plugin support

---

## 1. Build Components

```bash
# Envoy
cd /path/to/envoy
cargo build --release

# Atheneum
cd /path/to/atheneum
cargo build --release
```

---

## 2. Initialize Databases

```bash
# Atheneum (vector store)
/path/to/atheneum/target/release/atheneum init ~/.envoy/atheneum.db
```

Expected output:
```
✅ Graph initialized successfully
   Health: OK
```

---

## 3. Start Envoy Server

```bash
ATHENEUM_DB=~/.envoy/atheneum.db /path/to/envoy/target/release/envoy
```

You should see:
```
envoy server listening on 127.0.0.1:9876, db=~/.envoy/server.db
atheneum integration enabled: ~/.envoy/atheneum.db
```

---

## 4. Verify Installation

```bash
curl http://localhost:9876/health
```

Response:
```json
{
  "status": "ok",
  "atheneum_configured": true,
  "agents_online": 0
}
```

---

## 5. Send First Message

```bash
# Register an agent
AGENT_ID=$(curl -s -X POST http://localhost:9876/agents \
  -H "content-type: application/json" \
  -d '{"name":"test-agent","kind":"worker"}' | jq -r '.agent_id')

# Send a message
curl -X POST http://localhost:9876/messages \
  -H "content-type: application/json" \
  -d "{
    \"type\": \"direct\",
    \"from\": \"${AGENT_ID}\",
    \"to\": \"${AGENT_ID}\",
    \"parts\": [{\"text\": \"Hello, multi-agent world!\"}]
  }"

# Receive messages
curl "http://localhost:9876/messages?to=${AGENT_ID}&since=0"
```

---

## Common Commands

| Command | Purpose |
|---------|---------|
| `curl http://localhost:9876/health` | Check server health |
| `curl http://localhost:9876/agents` | List all agents |
| `curl http://localhost:9876/stats` | Message statistics |
| `curl "http://localhost:9876/messages?to=<id>&since=0"` | Poll for messages |

---

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `ENVOY_DB` | `~/.envoy/server.db` | Envoy database location |
| `ENVOY_PORT` | `9876` | HTTP server port |
| `ATHENEUM_DB` | (none) | Atheneum database for semantic search |

---

## Next Steps

- Read [MULTI_AGENT_SYSTEM_MANUAL.md](./MULTI_AGENT_SYSTEM_MANUAL.md) for full documentation
- Explore [Envoy API.md](../API.md) for complete API reference
- Check Atheneum documentation for vector search capabilities

---

## Troubleshooting

**Port 9876 already in use:**
```bash
lsof -i :9876
kill <PID>
```

**Atheneum not configured:**
```bash
rm ~/.envoy/atheneum.db
/path/to/atheneum/target/release/atheneum init ~/.envoy/atheneum.db
```

**Database locked:**
```bash
# Stop envoy, remove lock file, restart
pkill envoy
rm ~/.envoy/server.db-shm ~/.envoy/server.db-wal
ATHENEUM_DB=~/.envoy/atheneum.db envoy
```
