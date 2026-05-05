# Envoy Development Rules — Grounded Agent Workflow

**Project:** Envoy — Message/Coordination Server for AI Coding Agents
**Crate:** `envoy` | **Foundation:** `sqlitegraph` v2.1.3 (pub/sub)
**Last Updated:** 2026-05-05

---

## Shared Agent Workflow

Follow `/home/feanor/Projects/CLAUDE.md` for shared rules: state assumptions before coding, use magellan/llmgrep/mirage for code-structure claims, keep edits surgical, preserve dirty worktree changes, and report fresh verification evidence before claiming completion.

---

## Subagent Trust Model

**Subagents are NOT trusted by default.** Their output is only valid when:

1. SubagentStop hooks passed (all 7 gates green — the hooks BLOCK on exit code 2)
2. The subagent can cite specific magellan/llmgrep queries it ran BEFORE writing code
3. `cargo check` and `cargo test` pass on the subagent's changes
4. No stubs, placeholders, or `#[allow(dead_code)]` in the diff

**If SubagentStop hooks blocked the subagent, its summary is LIES.** Read the actual diff, fix the violations, and verify yourself. Never relay a blocked subagent's conclusion as truth.

**Parent agent responsibilities when using subagents:**
- Read the subagent's git diff before accepting its work
- Verify hook output (check for exit code 2 blocks)
- If the subagent didn't run magellan/llmgrep queries, its code is based on hallucination — reject it
- Run `cargo check && cargo test` yourself after the subagent completes

---

## 10 Non-Negotiable Rules

1. Query magellan/llmgrep/mirage BEFORE writing ANY code
2. Run quality gate AFTER writing ANY code
3. Use grounded wrappers when querying tools
4. No stubs (todo!/unimplemented!/panic! in non-test = blocking)
5. No `#[allow(dead_code)]` — remove unused code
6. Result<T> over unwrap/expect unless marked `// M-ALLOW` or `// M-UNWRAP`
7. `cargo fmt + cargo check + cargo test` must pass before "done"
8. Re-index after changes (`magellan watch`)
9. Check CI after push (`gh run list`)
10. No guessing. The database is truth. Memory is not.

---

## Quick Start

```bash
cargo build
cargo test
cargo fmt --check
```

---

## Architecture

```
src/
├── lib.rs                # Public API: EnvoyEngine, Channel, Event, Subscription
├── engine.rs             # Core pub/sub engine
├── db.rs                 # SQLite schema management (channels, events, subscriptions)
├── event.rs              # Event types and payload validation
├── cli.rs                # CLI command dispatch
├── main.rs               # Binary entry point
└── error.rs              # Error types
```

**Key design:** envoy wraps sqlitegraph's pub/sub with agent-coordination semantics. Every event carries a `magellan_trace` as verifiable proof of work.

---

## Database

**Location:** `.magellan/envoy.db`

Envoy creates its own tables on top of sqlitegraph's graph schema:

```sql
channels(id, name, description)
events(id, channel_id, sender, payload JSON, timestamp, sequence_id)
subscriptions(agent_id, channel_id, last_seen_sequence)
```

---

## Mandatory Pre-Code Queries

**Before ANY code change:**

```bash
# Check DB health
magellan status --db .magellan/envoy.db 2>/dev/null || \
    magellan watch --root . --db .magellan/envoy.db --scan-initial &

# Find existing symbols
magellan find --db .magellan/envoy.db --name "symbol_name"

# Check callers/callees
magellan refs --db .magellan/envoy.db --name "func" --path src/file.rs --direction in
magellan refs --db .magellan/envoy.db --name "func" --path src/file.rs --direction out

# Search for patterns
llmgrep --db .magellan/envoy.db search --query "pattern" --mode symbols

# Analyze control flow
mirage cfg --db .magellan/envoy.db --function "function_name"
```

---

## Mandatory Post-Code Verification

After ANY code change:

```bash
cargo fmt
cargo check
cargo test
.claude/scripts/quality-gate.sh --full
magellan watch --root . --db .magellan/envoy.db --scan-initial &
```

---

## Hook Chain (Automatic Enforcement)

| Trigger | Hook | Block on Exit 2 |
|---------|------|-----------------|
| SubagentStart | `query-schema-check.fish` | Yes — DB must be healthy |
| SubagentStop | `ci-check.fish` | No — warns only |
| SubagentStop | `query-symbol-check.fish` | Yes — must have queried before coding |
| SubagentStop | `verify-rust.fish` | Yes — fmt + check + graph |
| SubagentStop | `splice-cycles-check.fish` | Yes — no call graph cycles |
| SubagentStop | `stub-check.fish` | Yes — no panic!/todo! |
| SubagentStop | `wiring-check.fish` | No — warns on exit 1 |
| SubagentStop | `security-check.fish` | Yes |
| SessionEnd | `logseq-session-hook.fish` | No — never blocks |

---

## Code Quality Standards

**NO PLACEHOLDER CODE — EVER.** Forbidden: `todo!()`, `unimplemented!()`, `// TODO:`, `// FIXME:`, `// HACK:`, stubs, mocks, `#[allow(dead_code)]`, commented-out code. Implement properly or remove.

**Error handling:** `Result<T, EnvoyError>` over `unwrap()`/`expect()`. Mark intentional unwraps with `// M-ALLOW` or `// M-UNWRAP`.

**Event payloads** must include required fields: `status`, `working_on`, `waiting_for`, `can_start`, `verified`, `magellan_trace`.

---

## When to Re-Index

Only when: schema mismatch, first time, or `magellan status` shows 0 files/symbols.

```bash
pkill -f "magellan watch"
rm -rf .magellan/envoy.db*
magellan watch --root . --db .magellan/envoy.db --scan-initial
```

---

## Subagent Dispatch Rules

When dispatching a subagent in this project:

1. **Be specific about what queries to run** — name exact files and symbols
2. **Require evidence** — the subagent must report which queries it ran
3. **Check the diff yourself** — git diff the subagent's changes before accepting
4. **Re-run verification** — `cargo check && cargo test` after subagent completes
5. **If hooks blocked the subagent** — the work is REJECTED. Read the hook output, fix, and verify.
6. **Never chain subagents** — a second subagent cannot verify the first. Only the parent (you) can accept work.

---

## Anti-Patterns

- Never assume a function exists without querying magellan
- Never invent schema/column names — check the actual schema
- Never skip verification — "looks good" is not verification
- Never trust a subagent's self-reported success — check the hooks
- Never commit without `cargo test` passing
