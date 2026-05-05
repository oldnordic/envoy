# Envoy API Reference

Base URL: `http://127.0.0.1:9876`

All request and response bodies are JSON. Errors use the format:
```json
{ "error": { "code": "ERROR_CODE", "message": "human description" } }
```

---

## Agents

### `POST /agents` — Register an agent

**Request:**
```json
{
  "name": "claude",
  "kind": "claude",
  "parent_id": null
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Human-readable label (non-unique) |
| `kind` | string | yes | Agent platform: `"claude"`, `"hermes"`, etc. |
| `parent_id` | string \| null | no | Parent agent ID. Omit or `null` for root agents. |

**Response** `201 Created`:
```json
{
  "agent_id": "id1",
  "name": "claude",
  "kind": "claude",
  "parent_id": null,
  "online": true
}
```

Subagents get dot-notation IDs. If parent is `"id1"`, the first child is `"id1.1"`,
the second `"id1.2"`, and a grandchild is `"id1.1.1"`.

**Errors:** `409 AGENT_ALREADY_EXISTS`, `404 AGENT_NOT_FOUND` (parent), `409 AGENT_OFFLINE` (parent)

---

### `GET /agents` — List all agents

**Response** `200 OK`:
```json
{
  "agents": [
    {
      "agent_id": "id1",
      "name": "claude",
      "kind": "claude",
      "parent_id": null,
      "online": true
    },
    {
      "agent_id": "id2",
      "name": "hermes",
      "kind": "hermes",
      "parent_id": null,
      "online": true
    }
  ]
}
```

---

### `GET /agents/{agent_id}` — Get agent detail

**Response** `200 OK`:
```json
{
  "agent_id": "id1",
  "name": "claude",
  "kind": "claude",
  "parent_id": null,
  "online": true,
  "children": ["id1.1", "id1.2"]
}
```

**Errors:** `404 AGENT_NOT_FOUND`

---

### `DELETE /agents/{agent_id}` — Disconnect agent

Marks the agent and all descendants offline. Cascade is depth-first: disconnect
recurses through all children in the hierarchy tree.

**Response** `200 OK`:
```json
{
  "disconnected": true,
  "affected": ["id1", "id1.1", "id1.1.1"]
}
```

**Errors:** `404 AGENT_NOT_FOUND`

---

### `GET /agents/{agent_id}/messages/pending` — Tombstone endpoint

Returns undelivered messages for a disconnected agent. Messages are preserved even
after the agent goes offline.

**Response** `200 OK`:
```json
{
  "messages": [
    {
      "message_id": "3",
      "type": "handoff",
      "from": "id1.1",
      "to": "id1",
      "task_id": "task-003",
      "context_id": "ctx-001",
      "timestamp": "2026-05-05T22:49:09.353+00:00",
      "sequence_id": 1,
      "parts": [
        {"text": "context at 28%, handing off"},
        {"data": { "completion_status": "NEEDS_CONTEXT", ... }}
      ]
    }
  ],
  "count": 1
}
```

---

## Messages

### `POST /messages` — Send a message

**Request:**
```json
{
  "type": "direct",
  "from": "id1",
  "to": "id2",
  "task_id": null,
  "context_id": null,
  "parts": [
    {"text": "Hello, can you review PR #42?"},
    {"data": {"priority": "high"}},
    {"url": "https://github.com/org/repo/pull/42"}
  ]
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | `"direct"`, `"handoff"`, `"heartbeat"`, `"system"` |
| `from` | string | yes | Sender agent ID (must be online) |
| `to` | string | yes | Recipient agent ID (must exist) |
| `task_id` | string \| null | no | Optional task correlation |
| `context_id` | string \| null | no | Optional context session tracking |
| `parts` | Part[] | yes | At least 1, max 20 parts |

**Part types:**

| Variant | JSON | Example |
|---------|------|---------|
| Text | `{"text": "..."}` | `{"text": "hello"}` |
| Data | `{"data": {...}}` | `{"data": {"status": "working"}}` |
| Url | `{"url": "..."}` | `{"url": "https://..."}` |

**Response** `201 Created` — The stored message envelope with server-assigned fields:
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
    {"text": "Hello, can you review PR #42?"}
  ]
}
```

Server-assigned fields:
- `message_id` — Row ID from SQLite
- `timestamp` — RFC 3339 UTC timestamp
- `sequence_id` — Per-recipient monotonic sequence number

**Side effect:** If the recipient has an active WebSocket connection, the message
is pushed as a `{"event":"message","data":{...}}` frame.

**Errors:** `404 AGENT_NOT_FOUND` (sender or recipient), `409 AGENT_OFFLINE` (sender),
`400 INVALID_MESSAGE`, `400 MESSAGE_TOO_LARGE`, `400 TOO_MANY_PARTS`

---

### `GET /messages?to={agent_id}&since={seq}&limit={n}` — Poll messages

Cursor-based polling for a recipient's messages.

**Query parameters:**

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `to` | string | (required) | Recipient agent ID |
| `since` | integer | `0` | Return messages with `sequence_id > since` |
| `limit` | integer | `50` | Max messages to return (capped at 100) |

**Response** `200 OK`:
```json
{
  "messages": [
    {
      "message_id": "1",
      "type": "direct",
      "from": "id1",
      "to": "id2",
      "task_id": null,
      "context_id": null,
      "timestamp": "2026-05-05T22:48:57.592+00:00",
      "sequence_id": 1,
      "parts": [{"text": "Hello"}]
    }
  ],
  "latest_sequence": 1
}
```

Use `latest_sequence` as `since` in the next poll for continuous iteration:
```bash
# First poll
curl "http://127.0.0.1:9876/messages?to=id2&since=0"
# → latest_sequence: 1

# Next poll
curl "http://127.0.0.1:9876/messages?to=id2&since=1"
# → returns only messages with sequence_id > 1
```

**Errors:** `404 AGENT_NOT_FOUND` (recipient)

---

### `GET /messages/{message_id}` — Get a single message

**Response** `200 OK` — Full `MessageEnvelope` as above.

**Errors:** `404 MESSAGE_NOT_FOUND`

---

## WebSocket

### `GET /ws/{agent_id}` — WebSocket upgrade

Upgrades the HTTP connection to a WebSocket for real-time event push. The agent
must be registered and online.

**Events received by the client:**

#### 1. Catch-up messages (on connect)

All undelivered messages for the agent are sent first, up to 100:

```json
{
  "event": "message",
  "data": {
    "message_id": "3",
    "type": "handoff",
    "from": "id1.1",
    "to": "id1",
    "task_id": "task-003",
    "context_id": "ctx-001",
    "timestamp": "2026-05-05T22:49:09.353+00:00",
    "sequence_id": 1,
    "parts": [...]
  }
}
```

#### 2. Connected event

Sent after catch-up completes:

```json
{
  "event": "agent_connected",
  "data": {
    "agent_id": "id1"
  }
}
```

#### 3. Real-time messages

Pushed whenever a message is sent to this agent via `POST /messages`:

```json
{
  "event": "message",
  "data": { ... }
}
```

**Client-to-server:** Text frames are accepted as heartbeats (acknowledged
silently). The connection is kept open until either side closes.

**Errors:** `409 AGENT_OFFLINE` (HTTP response before upgrade)

---

## Health & Stats

### `GET /health`

**Response** `200 OK`:
```json
{
  "status": "ok",
  "uptime_seconds": 7243,
  "agents_online": 3
}
```

### `GET /stats`

**Response** `200 OK`:
```json
{
  "messages_total": 42,
  "agents_registered": 5
}
```

---

## Error Reference

| HTTP Status | Error Code | When |
|-------------|------------|------|
| 400 | `INVALID_MESSAGE` | Empty parts, bad handoff status |
| 400 | `MESSAGE_TOO_LARGE` | Text part > 1 MB |
| 400 | `TOO_MANY_PARTS` | More than 20 parts |
| 400 | `SERIALIZATION_ERROR` | Malformed JSON body |
| 404 | `AGENT_NOT_FOUND` | Agent ID doesn't exist |
| 404 | `MESSAGE_NOT_FOUND` | Message ID doesn't exist |
| 404 | `CHANNEL_NOT_FOUND` | Channel name/id doesn't exist |
| 409 | `AGENT_OFFLINE` | Agent is disconnected |
| 409 | `AGENT_ALREADY_EXISTS` | Duplicate agent name |
| 500 | `INTERNAL_ERROR` | Graph, database, or WebSocket infrastructure error |
