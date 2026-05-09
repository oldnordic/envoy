# Envoy Atheneum API Documentation

**Version:** 0.3.0  
**Base URL:** `http://127.0.0.1:9876`  
**Feature Flag:** `atheneum`  

All endpoints in this API are cfg-gated behind the `atheneum` feature. They are only available when envoy is built with `--features atheneum` and the `ATHENEUM_DB` environment variable is configured.

---

## Configuration

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `ENVOY_DB` | No | `/home/feanor/.envoy/server.db` | Path to envoy's SQLite database |
| `ENVOY_PORT` | No | `9876` | Port for HTTP server |
| `ATHENEUM_DB` | Yes (for atheneum) | - | Path to atheneum's SQLite database |

### Systemd Service Example

```ini
[Service]
Environment=ENVOY_DB=/home/feanor/.local/share/envoy/agents.db
Environment=ATHENEUM_DB=/home/feanor/.local/share/envoy/atheneum.db
ExecStart=/path/to/envoy serve --port 9876
```

---

## Endpoints

### 1. Store Discovery

Stores a discovery (symbol, CFG finding, pattern, issue) in atheneum for reuse by other agents.

**Endpoint:** `POST /atheneum/discoveries`

**Request:**
```json
{
  "agent": "string",           // Agent making the discovery
  "discovery_type": "string",  // Type: "Symbol", "Caller", "CFG", "Issue", "Pattern"
  "target": "string",          // Symbol or target name
  "metadata": {}               // Arbitrary JSON metadata
}
```

**Response (201 Created):**
```json
{
  "discovery_id": 18,
  "agent": "agent_name",
  "target": "symbol_name",
  "discovery_type": "Symbol"
}
```

**Error Responses:**
- `500` — Atheneum not configured or database error

**Example:**
```bash
curl -X POST http://127.0.0.1:9876/atheneum/discoveries \
  -H "Content-Type: application/json" \
  -d '{
    "agent": "claude-1",
    "discovery_type": "Symbol",
    "target": "build_router",
    "metadata": {
      "file": "src/http.rs",
      "line": 555,
      "signature": "pub fn build_router(state: SharedState) -> Router"
    }
  }'
```

---

### 2. Query Discoveries

Retrieves all discoveries for a specific target symbol.

**Endpoint:** `GET /atheneum/discoveries?target=<symbol>`

**Query Parameters:**
| Parameter | Required | Type | Description |
|-----------|----------|------|-------------|
| `target` | Yes | string | Symbol name to query |

**Response (200 OK):**
```json
{
  "target": "build_router",
  "discovery_count": 2,
  "discoveries": [
    {
      "id": 1,
      "name": "agent_a: build_router",
      "data": {
        "agent": "agent_a",
        "discovery_type": "Symbol",
        "file": "src/http.rs",
        "line": 555,
        "target": "build_router",
        "timestamp": "2026-05-09T20:00:00Z"
      }
    }
  ]
}
```

**Error Responses:**
- `400` — Missing `target` query parameter
- `500` — Atheneum not configured or database error

---

### 3. Create Handoff

Creates a handoff manifest for transferring work between agents.

**Endpoint:** `POST /atheneum/handoffs`

**Request:**
```json
{
  "from_agent": "string",    // Agent sending the handoff
  "to_agent": "string",      // Agent receiving the handoff
  "manifest": {}             // Handoff manifest (arbitrary JSON)
}
```

**Suggested Manifest Schema:**
```json
{
  "status": "DONE|DONE_WITH_CONCERNS|BLOCKED|NEEDS_CONTEXT",
  "context_remaining_pct": 50,  // 0-100
  "what_was_done": "string",
  "what_is_stubbed": ["string"],
  "remaining_work": ["string"],
  "verification_state": {
    "tests_passing": 10,
    "tests_failing": 0,
    "cargo_check_passed": true
  },
  "magellan_trace": {
    "files_changed": ["src/http.rs"],
    "symbols_added": ["atheneum_path"]
  },
  "grounded_queries_used": [
    "magellan find atheneum_path"
  ]
}
```

**Response (201 Created):**
```json
{
  "handoff_id": 21,
  "from_agent": "agent_a",
  "to_agent": "agent_b",
  "created_at": "2026-05-09T21:12:00.108719810Z"
}
```

**Error Responses:**
- `500` — Atheneum not configured or database error

---

### 4. Get Pending Handoff

Retrieves the oldest pending handoff for an agent.

**Endpoint:** `GET /atheneum/handoffs/pending?agent=<agent_name>`

**Query Parameters:**
| Parameter | Required | Type | Description |
|-----------|----------|------|-------------|
| `agent` | Yes | string | Agent name to query for pending handoffs |

**Response (200 OK):**
```json
{
  "handoff": {
    "id": 21,
    "name": "agent_a -> agent_b",
    "from_agent": "agent_a",
    "to_agent": "agent_b",
    "manifest": { ... },
    "created_at": "2026-05-09T21:12:00.196719367Z"
  }
}
```

**Response (200 OK) - No Handoff:**
```json
{
  "handoff": null
}
```

**Error Responses:**
- `400` — Missing `agent` query parameter
- `500` — Atheneum not configured or database error

---

### 5. Claim Handoff

Marks a handoff as claimed, removing it from the pending queue.

**Endpoint:** `POST /atheneum/handoffs/{id}/claim`

**Path Parameters:**
| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | integer | Handoff ID to claim |

**Response (200 OK):**
```json
{
  "claimed": true,
  "handoff_id": 21
}
```

**Error Responses:**
- `404` — Handoff not found
- `500` — Atheneum not configured or database error

---

### 6. Query Knowledge

Queries atheneum for all knowledge about a target, including token savings.

**Endpoint:** `GET /atheneum/knowledge?target=<symbol>`

**Query Parameters:**
| Parameter | Required | Type | Description |
|-----------|----------|------|-------------|
| `target` | Yes | string | Symbol name to query |

**Response (200 OK):**
```json
{
  "target": "build_router",
  "discovery_count": 2,
  "token_savings": {
    "total": 15000,
    "by_type": {
      "Symbol": 5000,
      "CFG": 10000
    }
  }
}
```

**Error Responses:**
- `400` — Missing `target` query parameter
- `500` — Atheneum not configured or database error

---

## Health Check

**Endpoint:** `GET /health`

**Response (200 OK):**
```json
{
  "status": "ok",
  "uptime_seconds": 2386,
  "agents_online": 0
}
```

**Note:** The health endpoint does NOT indicate whether atheneum is configured. To verify atheneum availability, attempt to query an atheneum endpoint.

---

## Error Response Format

All error responses follow this format:

```json
{
  "code": "error_code",
  "message": "Human readable error message"
}
```

**Common Error Codes:**
- `ATHENEUM_NOT_CONFIGURED` — ATHENEUM_DB environment variable not set
- `DATABASE_ERROR` — SQLite operation failed
- `DESERIALIZATION_ERROR` — Invalid JSON in request

---

## Discovery Types

Recommended values for `discovery_type`:

| Type | Description | Example Metadata |
|------|-------------|------------------|
| `Symbol` | Found symbol location, signature | `{file, line, signature}` |
| `Caller` | Who calls this symbol | `{callers: ["func1", "func2"]}` |
| `CFG` | Control flow analysis | `{complexity: 8, branches: [...]}` |
| `Issue` | Bug found | `{issue_type, severity, fix}` |
| `Pattern` | Design pattern found | `{pattern: "Strategy", location: ...}` |

---

## Handoff Completion Status

Recommended values for `manifest.status`:

| Status | Meaning |
|--------|---------|
| `DONE` | Work complete, no concerns |
| `DONE_WITH_CONCERNS` | Complete but has caveats |
| `BLOCKED` | Cannot proceed, needs input |
| `NEEDS_CONTEXT` | Reached token budget, handing off |

---

## Port Configuration Note

The default envoy port is **9876**, not 8080. When configuring agents or MCP servers, use:

```
ENVOY_URL=http://127.0.0.1:9876
```

The systemd service explicitly sets `--port 9876`. Do not assume port 8080.

---

## Feature Detection

To check if atheneum is available:

1. **Build-time check:** Check if envoy was built with the atheneum feature
2. **Runtime check:** Call any atheneum endpoint and handle `ATHENEUM_NOT_CONFIGURED` error

Example:
```bash
# Attempt to query knowledge (lightweight check)
response=$(curl -s "http://127.0.0.1:9876/atheneum/knowledge?target=test")
if echo "$response" | jq -e '.code == "ATHENEUM_NOT_CONFIGURED"' > /dev/null; then
  echo "Atheneum not available"
else
  echo "Atheneum available"
fi
```
