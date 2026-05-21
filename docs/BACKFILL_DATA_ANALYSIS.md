# Envoy Message Data Analysis

**Date:** 2026-05-10
**Purpose:** Understand message structure before designing Atheneum backfill

## Message Statistics

| Metric | Count |
|--------|-------|
| Total Messages | 309 |
| Direct Messages | 199 |
| System Messages | 110 |
| Handoff Messages | 0 |
| Agents Registered | 14 |

## Agent Registry

| Agent ID | Display Name | Kind | Online |
|----------|--------------|------|--------|
| id1 | alice | worker | 0 |
| id2 | bob | worker | 0 |
| id3 | alice | worker | 0 |
| id4 | bob | worker | 0 |
| id5 | smoke-test | worker | 0 |
| id6 | claude2 | worker | 1 |
| id7 | hermes | coordinator | 1 |
| id8 | claude1 | worker | 1 |
| id9 | tester | worker | 1 |
| id10 | hermes | coordinator | 1 |
| id11 | live-check | test | 1 |
| id12 | live-check-a | test | 0 |
| id13 | live-check-b | test | 0 |
| id14 | claude1 | worker | 1 |

**Note:** Some agent IDs are duplicates (alice, hermes, claude1 appear multiple times) — likely from restarts/re-registrations.

## Message Schema

### EnvoyMessage Entity Structure

```json
{
  "id": 702,
  "kind": "EnvoyMessage",
  "name": "msg-d0301299-be08-4a7e-8b03-c52a2d4ee896",
  "data": {
    "msg_type": "direct|system|handoff",
    "from": "agent_id",
    "to": "agent_id",
    "context_id": "optional_subject",
    "sequence_id": 123,
    "task_id": "optional_task_id",
    "timestamp": "2026-05-09T...",
    "parts": [
      {
        "text": "message content or JSON string"
      }
    ],
    "acked_by": ["agent_id", ...]  // for direct messages
  }
}
```

### Message Types

#### 1. Direct Message (199 count)

**Purpose:** Agent-to-agent communication

**Example:**
```json
{
  "msg_type": "direct",
  "from": "id7",
  "to": "id8",
  "context_id": "docs-reviewed",
  "parts": [
    {
      "text": "Claude1 — reviewed all three docs. Solid work..."
    }
  ],
  "acked_by": ["id8"]
}
```

**Extractable Knowledge:**
- Agent coordination patterns
- Code review feedback
- Task status updates
- Decisions made

#### 2. System Message (110 count)

**Purpose:** Hook results, CI status, system events

**Example:**
```json
{
  "msg_type": "system",
  "from": "envoy",
  "to": "id5",
  "context_id": "hook_event",
  "parts": [
    {
      "text": "{\"event_type\":\"hook_result\",\"hook_name\":\"verify-rust\",\"exit_code\":2,...}"
    }
  ]
}
```

**Extractable Knowledge:**
- Hook execution results
- CI/CD status
- Project health metrics
- Error patterns

#### 3. Handoff Message (0 count)

**Expected but not present in current data.**

Expected schema (from API docs):
```json
{
  "msg_type": "handoff",
  "from": "id1.1",
  "to": "id1",
  "parts": [
    {
      "data": {
        "completion_status": "DONE|NEEDS_CONTEXT|BLOCKED",
        "what_was_done": [...],
        "remaining_work": [...],
        "verification_state": {...}
      }
    }
  ]
}
```

## Transformation Logic Design

### Question: What becomes a Discovery vs Handoff in Atheneum?

| Envoy Message Type | Atheneum Entity | Reasoning |
|--------------------|-----------------|-----------|
| Direct msg with code findings | `Discovery` | Agent found symbols, issues, patterns |
| Direct msg with review feedback | `Discovery` | Knowledge about code quality |
| Direct msg with task status | `Discovery` | Progress tracking |
| System msg (hook result) | `Discovery` | Knowledge about build/test status |
| Handoff msg (when present) | `Handoff` | Context transfer between agents |

### Open Questions

1. **Agent ID deduplication** — Should we track "alice" (id1, id3) as one agent or separate sessions?

2. **Discovery target** — For direct messages without a specific symbol/file, what is the `target`?
   - Option: Use `context_id` as target
   - Option: Use "agent-coordination" as generic target
   - Option: Extract mentions of files/symbols from text

3. **Discovery type** — What `discovery_type` values make sense?
   - `code_review`
   - `task_status`
   - `hook_result`
   - `coordination_decision`
   - `error_pattern`

4. **System message parsing** — The `parts[0].text` contains JSON-as-string. Need to parse before extracting.

5. **Timestamp handling** — Messages have `timestamp`, should we preserve or use Atheneum's auto-timestamp?

## Next Steps

1. ✅ Document data structure (this file)
2. ⏳ Design transformation mapping (what fields → what Atheneum fields)
3. ⏳ Write Rust backfill binary
4. ⏳ Test with sample data
5. ⏳ Full backfill
6. ⏳ Verify with `query_knowledge()`

## Issues Found (No Hotfixes)

| Issue | Severity | Description | Fix Later |
|-------|----------|-------------|-----------|
| No handoff messages | Info | Expected to exist based on API docs, but count is 0 | Investigate if handoffs use different entity type |
| Agent ID duplication | Low | Same agent name has multiple IDs | Decide: track as one agent or separate sessions |
| JSON-in-JSON for system messages | Low | `parts[0].text` contains JSON string | Parse in backfill logic |
