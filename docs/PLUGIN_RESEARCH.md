# Envoy Plugin Research — Cross-Agent Plugin System Comparison

Date: 2026-05-08
Status: Research complete, implementation pending

---

## Goal

Build native plugins for Hermes, Claude Code, and Codex that integrate with Envoy
as the real-time message transport — replacing hooks and scripts with proper plugin APIs.

---

## Plugin System Comparison

### Hermes Plugin System

**Location:** `~/.hermes/plugins/<name>/`
**Manifest:** `plugin.yaml` + `__init__.py` with `register(ctx)` function

```yaml
# plugin.yaml
name: envoy-coordination
version: 1.0.0
description: "Native Envoy integration for agent coordination"
author: "team"
kind: standalone
provides_tools:
  - envoy_send
  - envoy_listen_status
  - envoy_status
hooks:
  - on_session_start
  - on_session_end
```

**Key features:**
- `ctx.register_tool()` — registers tools into the Hermes toolset (like spotify plugin does)
- `ctx.register_hook()` — registers lifecycle hooks
- Available hooks: `pre_tool_call`, `post_tool_call`, `pre_llm_call`, `post_llm_call`,
  `on_session_start`, `on_session_end`, `subagent_stop`, `pre_gateway_dispatch`
- Plugin runs Python code — full access to httpx/asyncio for Envoy HTTP/WS
- Auto-discovery from `~/.hermes/plugins/` (user plugins)
- Config-driven enable: `plugins: [envoy-coordination]` in config.yaml

**Tools we'd register:**
- `envoy_send(to, subject, body)` — send a message via Envoy HTTP
- `envoy_status()` — check which agents are online
- `envoy_listen_start()` / `envoy_listen_stop()` — manage the listener daemon
- `envoy_messages(since)` — poll for missed messages

**Hooks we'd register:**
- `on_session_start` — start the envoy-listen daemon in background
- `on_session_end` — gracefully stop the listener

---

### Claude Code Plugin System

**Location:** `~/.claude/plugins/<marketplace>/<name>/`
**Manifest:** `.claude-plugin/plugin.json`

```json
{
  "name": "envoy-coordination",
  "description": "Envoy transport for multi-agent coordination",
  "version": "1.0.0",
  "author": { "name": "team" }
}
```

**Hooks:** `hooks/hooks.json`
```json
{
  "hooks": {
    "PreToolUse": [{ "hooks": [{ "type": "command", "command": "..." }], "matcher": "..." }],
    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "..." }], "matcher": "..." }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "..." }] }],
    "Notification": [{ "hooks": [{ "type": "command", "command": "..." }] }]
  }
}
```

**Skills:** `skills/<name>/SKILL.md` — loaded as agent knowledge

**Key features:**
- Hooks run external commands (shell scripts, python) — NOT inline code
- Hook types: `PreToolUse`, `PostToolUse`, `Stop`, `Notification`, `SubagentStop`
- Hooks receive JSON on stdin: `{"tool_name": "...", "tool_input": {...}}`
- `CLAUDE_PLUGIN_ROOT` env var points to plugin directory
- Skills provide knowledge/instructions — agent follows them procedurally
- Monitor tool: background processes with stdout notifications
- `plugin.json` is minimal — skills and hooks directories are separate

**What we'd build:**
- `hooks/hooks.json` with:
  - `Notification` hook: runs a script that calls `envoy-send` to acknowledge messages
  - `Stop` hook: cleans up envoy-listen process
- `skills/envoy-coordination/SKILL.md`: instructions for Claude to use `envoy-send`
  CLI and `envoy-listen` daemon
- A `hooks/start-listener.sh` and `hooks/stop-listener.sh` for lifecycle
- The actual `envoy-send.mjs` and `envoy-listen.mjs` scripts bundled in the plugin

---

### Codex (OpenAI) Plugin System

**Location:** `~/.codex/plugins/cache/<marketplace>/<name>/<version>/`
**Manifest:** `.codex-plugin/plugin.json`

```json
{
  "name": "envoy-coordination",
  "version": "1.0.0",
  "description": "Envoy transport for multi-agent coordination",
  "author": { "name": "team", "email": "..." },
  "skills": "./skills/",
  "hooks": "./hooks.json",
  "mcpServers": "./.mcp.json"
}
```

**Key features:**
- `skills` field: relative path to skill directories (same SKILL.md format)
- `hooks` field: path to hooks config (similar to Claude Code)
- `mcpServers` field: can bundle an MCP server config!
- `apps` field: app manifest for integrations
- Skills provide procedural knowledge (agent follows instructions)
- Marketplace system with `marketplace.json` for discovery

**What we'd build:**
- `.codex-plugin/plugin.json` manifest
- `skills/envoy-coordination/SKILL.md`: instructions for Codex
- `hooks.json`: hooks for lifecycle management
- Bundle `envoy-send.mjs` and `envoy-listen.mjs` as scripts

---

## Architecture Decision

### Shared Scripts, Plugin-Specific Integration

The core logic (`envoy-send.mjs`, `envoy-listen.mjs`) is shared — same scripts,
same Envoy server. The plugin layer is agent-specific glue:

```
envoy/
├── scripts/                    # Shared — already built
│   ├── envoy-send.mjs
│   └── envoy-listen.mjs
├── hermes-plugin/              # Hermes native plugin
│   ├── plugin.yaml
│   ├── __init__.py             # register(ctx) — tools + hooks
│   └── tools.py                # Python handlers calling envoy HTTP API
├── claude-plugin/              # Claude Code plugin
│   ├── .claude-plugin/plugin.json
│   ├── hooks/hooks.json        # PreToolUse/Notification hooks
│   ├── hooks/start-listener.sh
│   ├── hooks/stop-listener.sh
│   └── skills/envoy/SKILL.md
├── codex-plugin/               # Codex plugin
│   ├── .codex-plugin/plugin.json
│   ├── hooks.json
│   └── skills/envoy/SKILL.md
└── systemd/                    # Service management
    └── envoy@.service          # Template unit for envoy server
```

### Implementation Order

1. **systemd service** for Envoy server (start/stop/status)
2. **Hermes plugin** — richest API (Python, tool registration, async hooks)
3. **Claude Code plugin** — hooks + skills
4. **Codex plugin** — skills + hooks

### What Gets Replaced

| Current (hooks/scripts) | Plugin equivalent |
|---|---|
| `~/.hermes/scripts/hermes-inbox-watchdog.sh` (cron) | Hermes plugin `on_session_start` starts listener |
| `~/.claude/hooks/` read-gate fish scripts | Stay as hooks (they're code-quality, not transport) |
| `/tmp/claude1_message_monitor.sh` (Monitor tool) | Claude plugin `Notification` hook + listener daemon |
| `/path/to/scripts/codex-message-monitor.sh` | Codex plugin skill + listener daemon |
| `scripts/envoy-send.mjs` (standalone) | Bundled inside each plugin |
| `scripts/envoy-listen.mjs` (standalone) | Bundled inside each plugin |

---

## Open Questions

1. **Envoy server lifecycle** — should it be a systemd user service, or should the Hermes plugin manage it (start on session_start, stop on session_end)?
2. **Agent ID management** — currently agents auto-register. Should there be a shared config file with fixed agent IDs to avoid re-registration on every restart?
3. **Message deduplication** — if an agent is offline and reconnects, catchup replay delivers missed messages. But if the listener was also writing to the file bus during that time, we could get duplicates. Need a dedup strategy (Envoy-Message-ID as primary key?).
4. **Claude Code plugin installation** — does it need to go through a marketplace, or can it be installed locally? Need to check `claude plugin install` CLI.
