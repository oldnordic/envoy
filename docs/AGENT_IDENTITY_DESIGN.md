# Agent Identity & Subagent Design Analysis

**Date:** 2026-05-10
**Context:** Designing agent identity for Envoy + Atheneum backfill

## Current Behavior

### Agent Registration (envoy-channel.js lines 121-144)

```javascript
async function registerWithName(name) {
  // Check if already registered
  if (agentCache.has(agentName)) {
    agentId = agentCache.get(agentName);
    // Reuse existing ID
    return true;
  }
  // Otherwise create new
  const resp = await httpPost("/agents", {
    name: agentName,
    kind: "worker",
    parent_id: null
  });
}
```

### Envoy Registry (agent.rs lines 142-152)

```rust
// Reclaim existing agent with same name if it exists (handles restart)
let existing_id = tree.agents.values()
    .find(|a| a.name == name)
    .map(|a| a.agent_id.clone());
if let Some(ref id) = existing_id {
    return self.reclaim(graph, id);  // Only works if OFFLINE
}
```

**Problem:** Reclaim only works if the old agent is OFFLINE. If session crashes, agent stays "online" → duplicate IDs.

## Subagent Architecture

### Claude Code Fork Subagents

From `/path/to/project

- Forks are **background workers** that inherit parent's full context
- They run in separate processes for parallel execution
- They are NOT separate "agents" in the user's mental model
- System prompt explicitly tells them: "You are a forked worker process. You are NOT the main agent."

### Current Envoy Subagent IDs

When `parent_id` is provided:
```rust
let agent_id = format!("{}.{}", pid, child_num);  // id1.1, id1.2, etc.
```

**Problem:** Fork subagents don't call `registerWithName()` — they inherit the parent's MCP tools but don't register separately.

## User's Mental Model

1. **I say "you are claude1"** → That agent should ALWAYS have the same ID
2. **Subagents of claude1** → Should use claude1's ID (not id1.1, id1.2)
3. **Purpose**: Consolidate knowledge under one agent identity

## Design Options

### Option A: Name-Based Identity (Preferred)

**Agent ID:** Use `name` as the canonical identifier, not numeric IDs

```rust
// Registration
pub fn register(&self, name: &str, kind: &str, parent_id: Option<String>) {
    let agent_id = name;  // "claude1", not "id8"
    // If "claude1" exists and is online → mark old offline, reclaim
    // If "claude1" doesn't exist → create new
}
```

**Subagents:** Use parent's `agent_id` directly

```rust
// Subagent of claude1
let agent_id = parent_id;  // Still "claude1"
```

**Pros:**
- Matches user's mental model
- Knowledge consolidated automatically
- No duplicate IDs for same name
- Subagents inherit parent identity naturally

**Cons:**
- Breaking change to current ID format
- Need migration for existing data

### Option B: Sticky IDs with Reclaim Logic

**Agent ID:** Keep numeric IDs but improve reclaim

```rust
pub fn register(&self, name: &str, kind: &str, parent_id: Option<String>) {
    // Find ANY agent with this name, even if online
    if let Some(existing) = tree.agents.values().find(|a| a.name == name) {
        if existing.online {
            // Force offline (crash recovery)
            self.disconnect(graph, &existing.agent_id)?;
        }
        return self.reclaim(graph, &existing.agent_id);
    }
    // ... create new
}
```

**Subagents:** Still use dot notation but store `canonical_name`

```rust
pub struct AgentInfo {
    pub agent_id: String,        // "id1.1"
    pub name: String,             // "subagent-1"
    pub canonical_name: String,   // "claude1" (inherited from parent)
    pub parent_id: Option<String>,
}
```

**Pros:**
- Less breaking change
- Keeps existing ID format
- Adds crash recovery

**Cons:**
- More complex identity resolution
- Still have ID vs name duality
- Subagents need special handling

### Option C: Hybrid (Recommended for Backfill)

**For backfill:** Normalize by `name`, ignore `agent_id`

```rust
// In backfill, group by name
let discoveries_by_agent: HashMap<&str, Vec<_>> = discoveries
    .into_iter()
    .group_by(|d| d.agent_name);

// "claude1" at id8 + id14 → merged as one agent
```

**For future:** Implement Option A (name-based identity)

## Recommendation

1. **Backfill (Phase 4):** Group by `name`, treat id8/id14 as same "claude1"
2. **Envoy fix (future):** Implement Option A with crash recovery
3. **Subagents:** Use parent's `agent_id` directly (no dot notation for forks)

## Open Questions

1. **Migration:** What to do with existing "id1.1" style subagent IDs in the database?
2. **Concurrency:** If two terminals both say "I am claude1", who wins?
3. **Atheneum schema:** Should we store `agent_name` or `agent_id` as the foreign key?

## Related

- [[envoy-atheneum-integration-gap]]
- [BACKFILL_DATA_ANALYSIS.md](./BACKFILL_DATA_ANALYSIS.md)
