# Envoy — Summary for Agent Continuation

## Project Goal

Build **envoy**: a message/coordination server for AI coding agents using sqlitegraph's pub/sub.

It replaces the current file-based message passing (markdown files in `Projects/messages/`) with a proper event-driven graph database system.

**Crate name:** `envoy` (available on crates.io, meaning "messenger/representative")

**Location:** `/home/feanor/Projects/envoy/`

## User Background

- **Luiz Cesar (oldnordic)** — Linux admin since 1996 (age 16), 29 years of systems thinking
- From Brazil, no PhD/CS degree (structural constraints, not lack of ability)
- Works after hours, solo, all code is LLM-written with him as architect/QA layer
- Builds for himself, shares free on GitHub/crates.io
- Philosophy: "The database is truth. Your memory is not."

## Existing Ecosystem (7 living crates)

| Crate | Purpose |
|-------|---------|
| **sqlitegraph** v2.1.3 | Dual-backend graph DB (SQLite + native V3), 35+ algorithms, HNSW vectors, pub/sub |
| **GeoMetriDB** | 3D spatial graph DB, dual octree, G3-A* pathfinding, CPU cache-optimized |
| **magellan** v3.1.9 | Live code indexer with bytespawn watcher, produces .db files (schema v12) |
| **llmgrep** | Structured semantic/structural code queries over magellan DBs |
| **mirage** | CFG analysis, path enumeration, hotspot detection |
| **splice** | Span-safe code editing + deterministic autocomplete (only from real symbols) |
| **odincode** | 1,037 tests, deterministic tool substrate |

### Project Layout Pattern

Every project has its own `.claude/`:
```
Projects/magellan/.claude/
Projects/llmgrep/.claude/
Projects/mirage/.claude/
Projects/splice/.claude/
Projects/sqlitegraph/.claude/
Projects/envoy/.claude/        (just created, copied from shared)
```

Shared canonical source: `Projects/.claude/` (hooks, scripts, quality-gate.sh, README)

## Multi-Agent Coordination System (Existing)

### Message Folder
```
Projects/messages/
├── claude/          (67 messages from Claude Code to Hermes)
├── hermes/          (65 messages from Hermes to Claude)
├── archive/
├── scripts/grounded-wrapper.sh
└── watch_claude.sh  (2-min cron polling watcher)
```

### Claude Code Setup
- Runs in-session with inotifywait watcher on `messages/hermes/`
- Configured per-project: settings.json hooks (SubagentStart/Stop/SessionEnd)
- 9 wired hooks, 6 subagents, 12 skills

### Hermes Setup
- Runs GLM-5.1 via Z.AI provider (glm-5-turbo)
- Watches `messages/claude/` via cron job every 2 minutes
- Has own hooks, grounded wrappers, Python pre_tool_call blocker
- Located at `~/.hermes/`

### Cross-Agent Infrastructure
- `~/.grounded/` — query-log.jsonl, session-log.jsonl, config.yaml
- Grounded wrappers: transparent proxies logging all tool calls
- `Projects/.remember/` — remember plugin for session extraction

## The Enforcement Architecture

### Hook Lifecycle
```
SubagentStart → query-schema-check.fish (block: DB must be healthy)
       ↓
    [agent works]
       ↓
SubagentStop → 7 sequential gates:
  1. ci-check.fish           (CI must be green)
  2. query-symbol-check.fish (must have queried graph before coding)
  3. verify-rust.fish        (fmt + check + status)
  4. splice-cycles-check.fish (no call graph cycles)
  5. stub-check.fish         (no panic!/todo!/unimplemented!)
  6. wiring-check.fish       (no dead modules, parse-once, debug cruft)
  7. security-check.fish     (no unsafe, SQL injection, secrets)
       ↓
SessionEnd → logseq-session-hook.fish (log session, never blocks)
```

### Quality Gate Script (quality-gate.sh)
8 checks: cargo fmt, cargo check, cargo test, stubs, unwrap/expect, dead_code allow, allow-without-reason, FIXME/HACK scan. Blocking vs warning per check. `--full` or `--diff` mode.

### Gate Bugs Found and Fixed
1. SKIPPED = PASS (critical lie) — scans printed PASS when they scanned zero files
2. cargo check ignored warnings (exits 0 even with warnings)
3. `#![allow(dead_code)]` not detected (crate-level suppressors missed)
4. "ALL CLEAN" ambiguous between "actually clean" and "did not scan"
5. `--full` flag advertised but not implemented
6. Arg parser broken (double-shift consumed subsequent flags)

### 10 Non-Negotiable Rules
1. Query magellan/llmgrep/mirage BEFORE writing ANY code
2. Run quality gate AFTER writing ANY code
3. Use grounded wrappers when querying tools
4. No stubs (todo!/unimplemented!/panic! in non-test = blocking)
5. No `#[allow(dead_code)]` — remove unused code
6. Result<T> over unwrap/expect unless marked M-ALLOW/M-UNWRAP
7. cargo fmt + cargo check + cargo test must pass before "done"
8. Re-index after changes (magellan watch)
9. Check CI after push (gh run list)
10. No guessing. The database is truth. Memory is not.

## The Pub/Sub Design (from brainstorm with Claude Web)

### Core Problem
Current polling-based file system has race conditions, silent gaps, agents going quiet. MCP is passive (model pulls), but coordination needs push (events fire on state change).

### Schema Design
```sql
-- channels: what topics exist
channels(id, name, description)

-- events: immutable, append-only message log
events(id, channel_id, sender, payload JSON, timestamp, sequence_id)

-- subscriptions: who gets what
subscriptions(agent_id, channel_id, last_seen_sequence)
```

### Required Message Payload Fields
Every event must carry:
```json
{
  "status": "working|waiting|blocked|done",
  "working_on": "specific task description",
  "waiting_for": "what I need or null",
  "can_start": "what other agent can begin now",
  "verified": true/false,
  "magellan_trace": {
    "files_changed": [],
    "symbols_added": [],
    "symbols_removed": [],
    "db_state": {"schema_version": 12, "symbol_count": 583}
  }
}
```

The `magellan_trace` field is the key differentiator — every coordination message carries proof of what actually changed, verifiable against the live index.

### Key Features Needed
- **Event-driven push** (not poll): subscribers notified when events fire
- **Sequence replay**: resuming agent catches up from last_seen_sequence
- **magellan traces** in every event as verifiable proof of work
- **Agent-agnostic**: Claude, Hermes, Codex, OpenCode, Copilot CLI all connect the same way
- **Context-handoff**: subagent context full → publish state → next agent replays and continues
- **Token savings**: structured queries replace full file reads (200 tokens vs 40,000)

### Future Scope
- Plugins for all major coding tools (Claude Code, Copilot, Cursor, Hermes, OpenCode)
- FFI for Java ecosystem (JNI/JNA)
- arxiv paper with benchmarks (before/after token counts, stub rates, coordination latency)

## What's Next
1. Scaffold Rust project: `cargo init --lib` in `/home/feanor/Projects/envoy/`
2. Add sqlitegraph dependency
3. Design database schema (channels, events, subscriptions tables)
4. Implement core pub/sub engine
5. Build CLI for agent integration
6. Wire into existing hook/skill infrastructure
