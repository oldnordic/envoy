# Shared Agent Standards — /home/feanor/Projects/.claude
#
# This directory contains agent-agnostic quality enforcement:
# hooks, scripts, and config used by BOTH Hermes and Claude Code.
#
# Both agents MUST reference these as the canonical source.
# Per-project .claude/hooks/ deployments symlink or copy from here.
#
# Layout:
#   hooks/     — fish scripts (Claude Code native, Hermes compatible via fish)
#   scripts/   — bash scripts (agent-agnostic quality gates)
#   agents/    — agent persona definitions
#   config/    — shared configuration

## CANONICAL HOOKS (source of truth)

All hooks in hooks/ are parametric — they derive project name and DB path
from CWD. No hardcoded paths. Deploy to any project by copying.

| Hook | Trigger | Blocks? | What it checks |
|------|---------|---------|----------------|
| query-schema-check.fish | SubagentStart | YES | DB must exist and be healthy |
| query-symbol-check.fish | SubagentStop | YES | magellan/llmgrep queries ran before code |
| verify-rust.fish | SubagentStop | YES | cargo fmt + cargo check + magellan status |
| stub-check.fish | SubagentStop | YES | No panic!/todo!/unimplemented! in non-test |
| security-check.fish | SubagentStop | YES | No unsafe, no SQL injection, no secrets |
| wiring-check.fish | SubagentStop | YES | No dbg!, no println! in lib code |
| splice-cycles-check.fish | SubagentStop | YES | No layer violations |
| ci-check.fish | SubagentStop | NO | Check GitHub CI after push |
| build-check.fish | SubagentStop | NO | Verify build artifacts |
| logseq-session-hook.fish | SessionEnd | NO | Log session to brain-dumps |

## CANONICAL SCRIPTS

| Script | Language | What it does |
|--------|----------|-------------|
| quality-gate.sh | bash | Standalone 8-check quality gate (cargo fmt/check/test + stub/unwrap/dead_code/allow scan) |
| quality-gate-hermes.fish | fish | Hermes-native quality gate (reads stdin JSON, outputs JSON context) |

## ENFORCEMENT MODEL

Claude Code: hooks wired in .claude/settings.json (SubagentStart/Stop/SessionEnd)
Hermes: hooks wired via Hermes hook system (subagent_stop)
Both: quality-gate.sh runnable standalone for manual verification

## NON-NEGOTIABLE RULES

1. Query magellan/llmgrep/mirage BEFORE writing ANY code
2. Run quality gate AFTER writing ANY code
3. No stubs (todo!/unimplemented!/panic!) in non-test code
4. No #[allow(dead_code)] — remove unused code
5. Result<T> over panic!/unwrap in non-test code
6. cargo fmt + cargo check + cargo test must pass
7. Re-index code graph after changes (magellan watch)
8. Log grounding queries to ~/.grounded/query-log.jsonl

## AGENTS REFERENCE

Claude: reads hooks/ from per-project .claude/hooks/ (deployed copies)
Hermes: reads hooks/ from ~/.hermes/hooks/hermes/ (deployed copies)
Source of truth: THIS directory. Changes here should be synced to both.
