# Envoy API Reference

Server: `http://localhost:9876` (default)
Binary: `ENVOY_DB=/path/envoy.db target/release/envoy`
Protocol: JSON over HTTP + WebSocket

---

## Authentication

None. Envoy is a local coordination server.

**IMPORTANT: Agent IDs vs Agent Names.** Agents are identified by `agent_id` (server-assigned, e.g. `"id1"`), NOT by display name (e.g. `"hermes"`). The `from`, `to`, `heartbeat.agent_id`, and `GET /messages?to=` fields all require `agent_id`. To resolve a name to an ID, call `GET /agents` and match on the `name` field. Clients should cache the name→ID mapping.

---

## Error Format

All errors return JSON:

```json
{
  "error": {
    "code": "ERROR_CODE",
    "message": "human-readable description"
  }
}
```

| HTTP | Code | When |
|------|------|------|
| 400 | `INVALID_MESSAGE` | Missing required fields, empty parts |
| 400 | `SERIALIZATION_ERROR` | Malformed JSON body |
| 400 | `TOO_MANY_PARTS` | More than 20 parts in message |
| 400 | `MESSAGE_TOO_LARGE` | Single text part exceeds 1MB |
| 404 | `AGENT_NOT_FOUND` | Agent ID does not exist |
| 404 | `MESSAGE_NOT_FOUND` | Message ID does not exist |
| 404 | `TASK_NOT_FOUND` | Task ID does not exist |
| 404 | `DEPENDENCY_NOT_FOUND` | Dependency ID does not exist |
| 404 | `PROJECT_CONFIG_NOT_FOUND` | No config for project |
| 404 | `SUBSCRIPTION_NOT_FOUND` | Agent not subscribed to project |
| 403 | `NOT_TASK_CLAIMANT` | Agent does not own the task |
| 409 | `AGENT_ALREADY_EXISTS` | Agent name already registered |
| 409 | `AGENT_OFFLINE` | Sender is not online |
| 409 | `TASK_ALREADY_CLAIMED` | Task taken by another agent |
| 409 | `INVALID_TASK_STATE` | Disallowed state transition |
| 409 | `DUPLICATE_DEPENDENCY` | Dependency already exists |
| 500 | `INTERNAL_ERROR` | Graph/DB error |

---

## Agents

### Register Agent

```
POST /agents
```

```json
{ "name": "claude2", "kind": "worker", "parent_id": null }
```

Response (`201`):
```json
{
  "agent_id": "id1",
  "name": "claude2",
  "kind": "worker",
  "online": true,
  "parent_id": null,
  "registered_at": "2026-05-08T06:00:00Z",
  "last_heartbeat_at": "2026-05-08T06:00:00Z",
  "status": { "state": "working", "working_on": "...", "checkpoint": "..." }
}
```

`agent_id` is server-assigned (auto-increment). Use this ID for all subsequent calls.

### List Agents

```
GET /agents
```

Response:
```json
{ "agents": [ { "agent_id": "id1", "name": "claude2", ... } ] }
```

### Get Agent

```
GET /agents/{agent_id}
```

Response includes `children` array of sub-agent IDs.

### Disconnect Agent

```
DELETE /agents/{agent_id}
```

Response:
```json
{ "disconnected": true, "affected": ["dep_id_1"] }
```

---

## Messages

### Send Message

```
POST /messages
```

```json
{
  "type": "direct",
  "from": "id1",
  "to": "id2",
  "task_id": null,
  "context_id": "status_update",
  "parts": [
    { "text": "WS client dogfood complete" }
  ]
}
```

| Field | Required | Description |
|-------|----------|-------------|
| `type` | yes | `direct`, `handoff`, `heartbeat`, `system` |
| `from` | yes | Sender agent_id (must be registered and online) |
| `to` | yes | Recipient agent_id (must be registered) |
| `task_id` | no | Task this message relates to |
| `context_id` | no | Thread/subject for grouping messages |
| `parts` | yes | Array of 1-20 content parts |

Part types:
- `{ "text": "body text" }` — text content (max 1MB each)
- `{ "data": { "key": "value" } }` — structured JSON
- `{ "url": "https://..." }` — URL reference

Response (`201`):
```json
{
  "message_id": "15",
  "type": "direct",
  "from": "id1",
  "to": "id2",
  "task_id": null,
  "context_id": "status_update",
  "timestamp": "2026-05-08T06:00:00Z",
  "sequence_id": 1,
  "parts": [ { "text": "WS client dogfood complete" } ]
}
```

If the recipient has an active WebSocket, the message is also pushed as:
```json
{ "event": "message", "data": { <full message envelope> } }
```

### Poll Messages

```
GET /messages?to={agent_id}&since={seq}&limit={n}
```

| Param | Required | Default | Description |
|-------|----------|---------|-------------|
| `to` | yes | — | Recipient agent_id |
| `since` | no | `0` | Return messages with sequence_id > this value |
| `limit` | no | `50` | Max messages (capped at 100) |

Response:
```json
{
  "messages": [ { "message_id": "15", ... } ],
  "latest_sequence": 15
}
```

**Polling pattern:**
1. First poll: `GET /messages?to=id2&since=0`
2. Store `latest_sequence` from response (e.g. `15`)
3. Next poll: `GET /messages?to=id2&since=15`
4. Repeat

### Get Message

```
GET /messages/{message_id}
```

Returns the full `MessageEnvelope`.

### Pending Messages (offline agent)

```
GET /agents/{agent_id}/messages/pending
```

Returns up to 100 undelivered messages for a disconnected agent. Useful for catch-up after reconnect.

---

## Heartbeat

### Send Heartbeat

```
POST /heartbeat
```

```json
{
  "agent_id": "id1",
  "status": {
    "state": "working",
    "task_id": null,
    "blocked_reason": null,
    "waiting_on_agent": null,
    "checkpoint": "implementation",
    "working_on": "building WS client"
  }
}
```

Agent states: `working`, `blocked`, `waiting_review`, `idle`

Response:
```json
{
  "accepted": true,
  "nudges": [
    { "reason": "Dependent claude1 may now be unblocked", "severity": "info" }
  ]
}
```

If no status is provided, a default `working` heartbeat is recorded.

Agents without heartbeat for `stale_threshold_minutes` (default: 5) are flagged as stale. The nudge loop sends warnings and reclaims their tasks.

**Note:** Envoy loads persisted agents as `online: false` on server restart. Clients should not blindly reuse an offline `agent_id` found by name after restart. Either re-register (creates new online agent) or send a heartbeat to bring the existing agent back online.

---

## WebSocket

### Connect

```
GET /ws/{agent_id}
```

Upgrade to WebSocket. The agent is marked online immediately.

### Server → Client Messages

All server pushes follow:
```json
{ "event": "<event_type>", "data": { ... } }
```

| Event | When |
|-------|------|
| `agent_connected` | On connect: `{ "agent_id": "id1" }` |
| `message` | New message for this agent |
| `hook_event` | Hook result event |
| `gate_event` | Quality gate event |
| `ci_event` | CI status event |
| `doc_event` | Doc sync event |
| `task_proposed` | New task created |
| `task_claimed` | Task claimed by agent |
| `task_state_changed` | Task state updated |
| `event_catchup` | Undelivered events replayed on reconnect |
| `channel_lagged` | Broadcast channel overflowed |
| `nudge` | Stale agent nudge |
| `blocker_stale` | A blocking agent may be stalled |
| `blocker_updated` | A blocking agent sent heartbeat |
| `dependency_resolved` | A dependency was resolved |
| `task_reclaimed` | Task reclaimed from stale agent |

### Client → Server Messages

**Heartbeat:**
```json
{
  "type": "heartbeat",
  "data": {
    "state": "working",
    "working_on": "building plugin",
    "checkpoint": "implementation"
  }
}
```
Response: `{ "type": "heartbeat_ack", "data": { "accepted": true, "timestamp": "..." } }`

**Ping:**
```json
{ "type": "ping" }
```
Response: `{ "type": "pong" }`

### Catch-up on Reconnect

When a WS connection opens for an agent that was previously connected:

1. Pending messages (from `MessageStore`) are replayed first
2. Undelivered events (from `DeliveryTracker`) are replayed as `event_catchup`
3. `agent_connected` event is sent last

---

## Event Bus

### Ingest Hook Event

```
POST /events/hook
```

```json
{
  "project": "envoy",
  "hook_name": "stub-check",
  "exit_code": 2,
  "output": "Found todo!() in src/lib.rs"
}
```

Severity mapping: exit_code 2 → `blocking`, non-zero → `warning`, 0 → `info`

### Ingest Gate Event

```
POST /events/gate
```

```json
{
  "project": "envoy",
  "gates_passed": 7,
  "gates_total": 8,
  "failures": ["wiring-check"]
}
```

### Ingest CI Event

```
POST /events/ci
```

```json
{
  "project": "envoy",
  "run_id": "25459445036",
  "status": "completed",
  "conclusion": "success",
  "head_branch": "master",
  "display_title": "CI run"
}
```

Conclusion `success` → `info`, `failure` → `blocking`, other → `info`

### Ingest Doc Event

```
POST /events/doc
```

```json
{
  "project": "envoy",
  "doc_files": ["CHANGELOG.md"],
  "last_updated_seconds": 3600
}
```

Over 86400s (24h) → `warning`, otherwise → `info`

### Query Events

```
GET /events?project={name}&since={timestamp}&limit={n}
```

| Param | Required | Default | Description |
|-------|----------|---------|-------------|
| `project` | yes | — | Project to filter by |
| `since` | no | — | RFC3339 timestamp, return events after this |
| `limit` | no | `50` | Max events (capped at 100) |

Response:
```json
{ "events": [ { "id": "5", "project": "envoy", ... } ], "count": 1 }
```

---

## Task Board

### Propose Task

```
POST /tasks/propose
```

```json
{ "project": "envoy", "description": "Build WS client", "blocked_by": [] }
```

### Claim Next Available

```
POST /tasks/claim-next
```

```json
{ "project": "envoy", "agent_id": "id1" }
```

Claims the oldest `proposed` task in the project, ordered by `created_at`.

### Claim Specific Task

```
POST /tasks/{id}/claim
```

```json
{ "agent_id": "id1" }
```

### Update Task State

```
POST /tasks/{id}/state
```

```json
{ "state": "in_progress", "checkpoint": "implementation" }
```

States: `proposed` → `claimed` → `in_progress` → `waiting_review` → `done`
Also: `claimed` → `proposed` (release), `in_progress` → `proposed` (release)

When a task moves to `done`, all tasks blocked by it receive a `dependency_resolved` event.

### Get Task

```
GET /tasks/{id}
```

### List Tasks

```
GET /tasks?project={name}&state={state}
```

---

## Subscriptions

### Subscribe Agent to Project

```
POST /subscriptions
```

```json
{ "agent_id": "id1", "project": "envoy" }
```

Subscribed agents receive project events via WebSocket broadcast.

### Unsubscribe

```
DELETE /subscriptions/{agent_id}/{project}
```

### List Subscriptions

```
GET /subscriptions/{agent_id}
```

Response:
```json
{ "agent_id": "id1", "subscriptions": ["envoy", "magellan"] }
```

---

## Project Config

### Get Config

```
GET /projects/{name}/config
```

### Set Config

```
POST /projects/{name}/config
```

```json
{
  "project": "envoy",
  "ci_poll_seconds": 60,
  "doc_poll_seconds": 300,
  "ci_repo_owner": "oldnordic/envoy",
  "doc_files": ["CHANGELOG.md"]
}
```

---

## Dependencies

### Create Dependency

```
POST /dependencies
```

```json
{
  "dependent_agent": "id1",
  "blocker_agent": "id2",
  "reason": "waiting for WS reconnect fix"
}
```

### Get Dependencies

```
GET /dependencies/blocker/{agent_id}    — what this agent blocks
GET /dependencies/dependent/{agent_id}  — what blocks this agent
```

### Resolve Dependency

```
POST /dependencies/{dep_id}/resolve
```

---

## Health & Stats

### Health Check

```
GET /health
```

```json
{ "status": "ok", "uptime_seconds": 3600, "agents_online": 3 }
```

### Stats

```
GET /stats
```

```json
{ "messages_total": 42, "agents_registered": 4 }
```

### Nudge Config

```
GET /nudge-config
POST /nudge-config
```

```json
{ "stale_threshold_minutes": 5, "check_interval_seconds": 30 }
```

---

## Proposed Additions (not yet implemented)

These endpoints are needed for proper plugin support:

### Acknowledge Message

```
POST /messages/{id}/ack
```

Marks a message as delivered/read. Prevents re-delivery on polling. Returns the updated message with a `delivered_at` timestamp.

### List Online Agents

```
GET /agents/online
```

Returns only agents currently marked online (useful for `envoy_who` tool).
