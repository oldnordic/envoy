# Envoy

HTTP+JSON coordination server for AI coding agents. Provides agent identity, structured messaging, session accountability, and knowledge persistence via the [atheneum](https://github.com/oldnordic/atheneum) graph database.

Part of the **grounded-coding ecosystem**. One-liner install via [grounded-coding](https://github.com/oldnordic/grounded-coding).

## Ecosystem

```
grounded-coding ── install + Claude Code plugin (skills, hooks, MCP)
       │
       ├── magellan   ── code graph indexer (symbols, call graph, CFG)
       ├── llmgrep    ── semantic code search over magellan graphs
       ├── mirage     ── CFG analysis (paths, loops, dominance)
       ├── splice     ── span-safe refactoring
       │
       ├── envoy      ◀── YOU ARE HERE (HTTP coordination server)
       │     └── atheneum  ── embedded knowledge graph (sessions, discoveries, tasks)
       │           └── sqlitegraph  ── SQLite graph engine
       │
       └── envoy-hook ── Claude Code hook binary (session + tool call logging)
```

Each tool is independently useful. Together they give LLM coding agents the infrastructure that vendors don't ship: persistent identity, audit trails, cross-session memory, and multi-agent coordination.

## Install

```bash
cargo install grounded-envoy   # installs both `envoy` server and `envoy-hook` binary
```

Or via the grounded-coding installer (recommended — also installs magellan, llmgrep, mirage, splice):

```bash
curl -fsSL https://raw.githubusercontent.com/oldnordic/grounded-coding/master/install.sh | sh
```

## Quick Start

```bash
# Start server (default: 127.0.0.1:9876)
envoy serve --port 9876

# Custom database path
ATHENEUM_DB=~/.local/share/atheneum/atheneum.db envoy serve

# Health check
curl http://127.0.0.1:9876/health
```

As a systemd user service (Linux):

```bash
systemctl --user start envoy
systemctl --user status envoy
```

## What It Does

Envoy gives AI coding agents what LLM vendors don't ship:

| Missing capability | Envoy provides |
|-------------------|----------------|
| No persistent identity | Named agents (`claude-main`, `claudesub1`) with parent/child hierarchy |
| No cross-session memory | Session history queryable by project — `GET /atheneum/sessions?project=X&last=3` |
| No audit trail | Every tool call logged: who, what, input/output summary, latency |
| No subagent accountability | Parent/child sessions, handover notes written on stop |
| No knowledge sharing | Discoveries, decisions, task state persisted across sessions |
| No multi-agent coordination | Pub/sub messaging, tasks, circuit breakers, dependencies |

## Core Concepts

### Agent Identity

Agents register on session start and get a server-assigned ID:

```bash
curl -X POST http://127.0.0.1:9876/agents \
  -H "content-type: application/json" \
  -d '{"name":"claude-main","kind":"claude"}'
# → {"agent_id":"id1","name":"claude-main","is_new":true,...}
```

IDs are hierarchical: root agents get `id1`, `id2`, ... Subagents get `id1.1`, `id1.2`, `id1.1.1`, etc. Explicitly retired IDs are reused. All calls require `X-Agent-Id` header.

### Session Accountability

Every Claude Code session writes to envoy automatically via `envoy-hook`:

```
SessionStart   → POST /atheneum/sessions        (project, branch, model, parent)
PostToolUse    → POST /atheneum/tool-calls       (tool, input summary, output summary, latency)
SubagentStop   → POST /atheneum/sessions/{id}/handover  (git diff, outcome)
Stop           → PATCH /atheneum/sessions/{id}   (tool count, cost, exit status)
```

Query prior state before acting:

```bash
curl "http://127.0.0.1:9876/atheneum/sessions?project=my-project&last=3"
```

Returns compact session history: branch, tool call count, file writes, last action.

### Knowledge Persistence

Store discoveries so future agents don't re-discover:

```bash
curl -X POST http://127.0.0.1:9876/atheneum/discoveries \
  -H "X-Agent-Id: $GROUNDED_AGENT_ID" \
  -H "content-type: application/json" \
  -d '{"agent":"claude","discovery_type":"Bug","target":"query_sessions",
       "metadata":{"file":"evidence.rs","line":547,"why":"anonymous ? params required"}}'
```

Query on next session start:

```bash
curl "http://127.0.0.1:9876/atheneum/context?project=my-project&limit=6"
```

## API Overview

### Agent Coordination

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/agents` | Register agent (idempotent — returns existing if name matches) |
| `GET` | `/agents` | List all agents |
| `GET` | `/agents/{id}` | Get agent + children |
| `DELETE` | `/agents/{id}` | Retire agent (cascades to children, ID goes to reuse pool) |
| `POST` | `/heartbeat` | Keep agent alive |
| `POST` | `/messages` | Send direct message |
| `GET` | `/messages` | Poll messages |
| `GET` | `/ws/{agent_id}` | WebSocket push |
| `GET` | `/health` | Health + uptime |

### Session Accountability (`--features atheneum`)

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/atheneum/sessions` | Record session start |
| `PATCH` | `/atheneum/sessions/{id}` | Record session end |
| `GET` | `/atheneum/sessions` | Query recent sessions (`?project=X&last=N`) |
| `POST` | `/atheneum/sessions/{id}/handover` | Write subagent handover note |
| `POST` | `/atheneum/tool-calls` | Record tool call |
| `GET` | `/atheneum/events` | Query event log |
| `GET` | `/atheneum/context` | Recent project discoveries (`?project=X&limit=N`) |
| `POST` | `/atheneum/discoveries` | Store discovery |
| `GET` | `/atheneum/knowledge` | Query knowledge by target |
| `GET` | `/atheneum/search` | Semantic search |
| `POST` | `/atheneum/tasks` | Create task |
| `GET` | `/atheneum/tasks` | List tasks |

Full API: [API.md](API.md)

## `envoy-hook` Binary

Standalone binary for Claude Code hooks. Reads session context from environment and stdin, posts to envoy.

```bash
envoy-hook session-start    # SessionStart hook  — registers agent, writes GROUNDED_AGENT_ID
envoy-hook tool-call        # PostToolUse hook   — logs tool call
envoy-hook session-end      # Stop hook          — patches session
envoy-hook subagent-end     # SubagentStop hook  — patches session + writes git diff handover
```

Wire in `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart":  [{"hooks":[{"type":"command","command":"envoy-hook session-start","timeout":5}]}],
    "SubagentStart": [{"hooks":[{"type":"command","command":"envoy-hook session-start","timeout":5}]}],
    "PostToolUse":   [{"hooks":[{"type":"command","command":"envoy-hook tool-call","timeout":5}]}],
    "Stop":          [{"hooks":[{"type":"command","command":"envoy-hook session-end","timeout":5}]}],
    "SubagentStop":  [{"hooks":[{"type":"command","command":"envoy-hook subagent-end","timeout":5}]}]
  }
}
```

## Configuration

| Env Var | Default | Purpose |
|---------|---------|---------|
| `ENVOY_DB` | `~/.local/share/envoy/agents.db` | Agent registry SQLite path |
| `ATHENEUM_DB` | `~/.local/share/atheneum/atheneum.db` | Knowledge graph SQLite path |
| `ENVOY_PORT` | `9876` | HTTP listen port |
| `ENVOY_URL` | `http://127.0.0.1:9876` | Used by `envoy-hook` and MCP server |

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `default` | ✓ | Includes `atheneum` — all session + knowledge endpoints |
| `atheneum` | ✓ | Atheneum bridge (`/atheneum/*` endpoints) |

## Requirements

- Rust 1.75+
- SQLite (bundled via rusqlite)

## Related

- [atheneum](https://github.com/oldnordic/atheneum) — embedded graph DB (used by envoy for knowledge storage)
- [grounded-coding](https://github.com/oldnordic/grounded-coding) — Claude Code plugin (skills + hooks using envoy)
- [magellan](https://github.com/oldnordic/magellan) — code graph indexer
- [sqlitegraph](https://crates.io/crates/sqlitegraph) — SQLite graph engine

## License

GPL-3.0-only — see [LICENSE](LICENSE).
