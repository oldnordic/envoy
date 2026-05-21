# Transformation Mapping: Envoy → Atheneum

**Date:** 2026-05-10
**Purpose:** Define how envoy messages map to Atheneum entities for backfill

## Mapping Overview

| Envoy Message | Atheneum Entity | Atheneum Method |
|---------------|----------------|-----------------|
| Direct message | `Discovery` | `store_discovery()` |
| System message | `Discovery` | `store_discovery()` |
| Handoff message | `Handoff` | `store_handoff()` |

**Note:** No handoff messages found in current data (0 count), but mapping included for completeness.

## Field Mappings

### Direct Message → Discovery

```rust
// Input: EnvoyMessage
{
  "msg_type": "direct",
  "from": "id8",           // → agent (merge by name: "claude1")
  "to": "id6",             // → not used (coordination is discovery)
  "context_id": "docs-reviewed",  // → target
  "parts": [{
    "text": "content..."  // → metadata.content
  }],
  "timestamp": "2026-05-09T...",
  "sequence_id": 123
}

// Output: Atheneum Discovery
store_discovery(
  agent: "claude1",              // from display_name (id8 → claude1)
  discovery_type: "coordination", // derived from context
  target: "docs-reviewed",        // context_id
  metadata: {
    "content": "message text",
    "timestamp": "2026-05-09T...",
    "sequence_id": 123,
    "to_agent": "claude2",        // id6 → claude2
    "original_msg_id": "msg-uuid"
  }
)
```

### System Message → Discovery

```rust
// Input: EnvoyMessage
{
  "msg_type": "system",
  "from": "envoy",
  "to": "id5",
  "context_id": "hook_event",
  "parts": [{
    "text": "{\"event_type\":\"hook_result\",\"hook_name\":\"verify-rust\",...}"  // JSON string
  }],
  "timestamp": "2026-05-09T..."
}

// Output: Atheneum Discovery
store_discovery(
  agent: "envoy",                // from
  discovery_type: "hook_result", // parsed from parts[0].text
  target: "verify-rust",         // parsed: hook_name
  metadata: {
    "event_type": "hook_result",
    "exit_code": 2,
    "project": "magellan",
    "timestamp": "2026-05-09T...",
    "to_agent": "smoke-test"      // id5 → display_name
  }
)
```

**Important:** System messages have JSON-as-string in `parts[0].text`. Must parse before extracting.

### Handoff Message → Handoff

```rust
// Input: EnvoyMessage (not found in data, but defined in API)
{
  "msg_type": "handoff",
  "from": "id1.1",
  "to": "id1",
  "parts": [{
    "data": {
      "completion_status": "DONE",
      "what_was_done": [...],
      "remaining_work": [...],
      "verification_state": {...}
    }
  }]
}

// Output: Atheneum Handoff
store_handoff(
  from_agent: "claude1",      // id1.1 → parent's display_name
  to_agent: "claude1",        // id1 → display_name (handing back to self)
  manifest: {
    "completion_status": "DONE",
    "what_was_done": [...],
    "remaining_work": [...],
    "verification_state": {...}
  }
)
```

## Discovery Type Taxonomy

Derived from `context_id` and message content:

| Context Pattern | Discovery Type | Target |
|-----------------|----------------|--------|
| `*_review*` | `code_review` | context_id |
| `hook_event` | `hook_result` | parsed hook_name |
| `*_push*`, `*_commit*` | `git_operation` | context_id |
| `*_docs*`, `*_doc*` | `documentation` | context_id |
| `re:` prefix | `response` | context_id |
| (default) | `coordination` | context_id |

## Agent Name Resolution

From `AGENT_IDENTITY_DESIGN.md` — merge by display name:

```rust
fn resolve_agent_name(agent_id: &str, agents: &HashMap<String, AgentInfo>) -> String {
    // Look up display_name
    if let Some(info) = agents.get(agent_id) {
        return info.display_name.clone(); // "claude1", not "id8"
    }
    // Fallback to agent_id itself
    agent_id.to_string()
}
```

**Mapping table (from current data):**

| Agent ID | Display Name | Use in Atheneum |
|----------|--------------|-----------------|
| id8 | claude1 | "claude1" |
| id14 | claude1 | "claude1" (merged) |
| id6 | claude2 | "claude2" |
| id7 | hermes | "hermes" |
| id10 | hermes | "hermes" (merged) |
| envoy | (system) | "envoy" |

## Metadata Schema

### Common fields (all discoveries)

```json
{
  "timestamp": "2026-05-09T...",
  "original_msg_id": "msg-uuid",
  "sequence_id": 123,
  "from_agent_id": "id8",      // preserved for traceability
  "to_agent_id": "id6"         // preserved for traceability
}
```

### Direct message additions

```json
{
  "content": "full message text",
  "to_agent": "claude2",        // resolved display name
  "context_id": "docs-reviewed"
}
```

### System message additions

```json
{
  "event_type": "hook_result",
  "hook_name": "verify-rust",
  "exit_code": 2,
  "project": "magellan",
  "severity": "blocking",
  "to_agent": "smoke-test"
}
```

## Special Cases

### 1. Empty context_id

Use `target` = `"agent-coordination"` (generic fallback)

### 2. Malformed JSON in system messages

Skip message, log error, continue with next

### 3. Duplicate discoveries

Same agent + same target + same discovery_type → update existing, don't create new

### 4. Very long message content

Truncate to 10000 chars, add `"truncated": true` to metadata

## Implementation Pseudocode

```rust
fn backfill_messages(envoy_db: &SqliteGraph, atheneum: &AtheneumGraph) -> Result<Stats> {
    // 1. Load all agents for name resolution
    let agents = load_agents(envoy_db)?;

    // 2. Load all messages
    let messages = load_messages(envoy_db)?;

    let mut stats = Stats::default();

    for msg in messages {
        match msg.msg_type {
            MessageType::Direct => {
                let from_name = resolve_name(&msg.from, &agents);
                let discovery_type = classify_direct_message(&msg);
                let target = msg.context_id.as_deref().unwrap_or("agent-coordination");

                let metadata = json!({
                    "content": msg.parts[0].text,
                    "timestamp": msg.timestamp,
                    "original_msg_id": msg.name,
                    "to_agent": resolve_name(&msg.to, &agents),
                    "context_id": msg.context_id
                });

                atheneum.store_discovery(&from_name, &discovery_type, target, metadata)?;
                stats.direct_processed += 1;
            }

            MessageType::System => {
                // Parse JSON from parts[0].text
                let event_data: Value = serde_json::from_str(&msg.parts[0].text)?;

                let discovery_type = event_data["event_type"].as_str().unwrap_or("system_event");
                let target = event_data["hook_name"].as_str()
                    .or_else(|| event_data["project"].as_str())
                    .unwrap_or("system");

                let mut metadata = event_data;
                metadata["timestamp"] = json!(msg.timestamp);
                metadata["original_msg_id"] = json!(msg.name);
                metadata["to_agent"] = json!(resolve_name(&msg.to, &agents));

                atheneum.store_discovery("envoy", discovery_type, target, metadata)?;
                stats.system_processed += 1;
            }

            MessageType::Handoff => {
                // Not found in current data, but handle for completeness
                let from_name = resolve_name(&msg.from, &agents);
                let to_name = resolve_name(&msg.to, &agents);

                if let Some(handoff_data) = extract_handoff_data(&msg) {
                    atheneum.store_handoff(&from_name, &to_name, handoff_data)?;
                    stats.handoffs_processed += 1;
                }
            }
        }
    }

    Ok(stats)
}
```

## Verification

After backfill, verify:

```bash
# 1. Count entities
sqlite3 ~/.envoy/atheneum.db "SELECT kind, COUNT(*) FROM graph_entities GROUP BY kind;"

# 2. Query specific discovery
curl "http://localhost:9876/atheneum/query?target=docs-reviewed"

# 3. Verify token savings calculation
curl "http://localhost:9876/atheneum/query?target=verify-rust"
```

Expected results:
- `Discovery` entities > 0
- `query?target=X` returns discoveries
- `token_savings.percentage_reduction` > 0

## Next Steps

1. ✅ Transformation mapping defined (this file)
2. ⏳ Write Rust backfill binary
3. ⏳ Test with sample data
4. ⏳ Full backfill execution
5. ⏳ Verification

## Related

- [BACKFILL_DATA_ANALYSIS.md](./BACKFILL_DATA_ANALYSIS.md) — Source data structure
- [AGENT_IDENTITY_DESIGN.md](./AGENT_IDENTITY_DESIGN.md) — Agent identity handling
- [envoy-atheneum-integration-gap](../../wiki/concepts/envoy-atheneum-integration-gap.md) — Original problem
