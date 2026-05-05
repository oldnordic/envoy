---
name: code-intelligence-ecosystem
description: Code intelligence tool stack including envoy, magellan, llmgrep, mirage, splice, sqlitegraph, odincode, and geometric_db. Grounded truth, zero guessing, verifiable operations.
---

# Code Intelligence Ecosystem

User has built a coherent stack of code intelligence tools with shared philosophy: **grounded truth, zero guessing, verifiable operations**.

## Living Projects

| Project | Version | Purpose | Location |
|---------|---------|---------|----------|
| **envoy** | v0.1.0 (new) | **THIS PROJECT** — Message/coordination server for AI coding agents using sqlitegraph pub/sub. Replaces file-based message passing with event-driven graph database system. | `/home/feanor/Projects/envoy/` |
| **sqlitegraph** | v2.1.3 | **FOUNDATION for envoy** — Published crates.io package. Dual backend (SQLite + native V3), 35+ graph algorithms, HNSW vector search, pub/sub engine used by envoy. | `/home/feanor/Projects/sqlitegraph/` |
| **magellan** | v3.1.9 | Deterministic codebase indexer (AST, symbols, CFG, calls). **FTS5 full-text search** (schema v12, 2.5× faster prefix queries). | `/home/feanor/Projects/magellan/` |
| **llmgrep** | v3.1.7 | Semantic code search on magellan databases. Supports schema v12 (FTS5 index). | `/home/feanor/Projects/llmgrep/` |
| **mirage** | v1.2.5 | Control-flow analysis (paths, dominance, loops, dead code). | `/home/feanor/Projects/mirage/` |
| **splice** | v2.6.2 | Span-safe refactoring (byte-accurate edits, cross-file rename) | `/home/feanor/Projects/splice/` |
| **odincode** | v0.0.1 | Deterministic tool substrate for LLM refactoring (1,037 tests) | `/home/feanor/Projects/odincode/` |
| **GeoMetriDB** (geometric_db_concept) | Research prototype | 3D spatial graph DB, dual octree, G3-A* pathfinding, MVCC temporal queries. | `/home/feanor/Projects/geometric_db_concept/` |

## Dead Projects (Do Not Pursue)

- **SynCore** — GPU inference pipeline, Candle GGUF, MCP server (abandoned)
- **LTMS** — Revolutionary transformation, centralized DB refactoring (abandoned)
- **LTMC** — Long-Term Memory Consolidation, Python MCP server (abandoned — MCP proved to be dead end)
- **CodeGraph** — Earlier graph iteration before sqlitegraph (abandoned)

Pattern: Ideas evolved into simpler, more focused tools. Don't revive dead projects; build on living ones.

## Critical Lesson: Why Optional Tools Fail

**MCP (Model Context Protocol) and similar optional tool systems are dead ends for grounded coding.** User proved this through LTMC experience.

**The Problem:**
- Optional tools are OPTIONAL — model must choose to call them (can ignore and hallucinate anyway)
- One-way flow — query → answer (no enforcement, no gates)
- No hooks — cannot BLOCK unverified code changes
- Model incentives fight truth — "be helpful/fast/confident" overrides "query first"

**What Actually Works:**
- **SKILLS (not tools)** — workflow encoded in context, loaded automatically every session
- **ENFORCEMENT LAYER** — hooks that FORCE query before any code write
- **PRE-CODE GATES** — cannot write file without magellan/llmgrep query
- **POST-CODE VERIFICATION** — code must compile, symbols must exist, tests must pass
- **HONESTY PROTOCOL** — "I don't know" = ACCEPTABLE, "let me query" = EXPECTED, "I'll guess" = REJECTED

**Key Insight:** The problem isn't tools — it's INCENTIVES. LLMs are trained to be helpful, confident, and fast. This fights against verification, uncertainty, and query-first workflows. You must ENFORCE truth, not suggest it.

## Release Discipline — GitHub First, Then crates.io

**CRITICAL: Never publish to crates.io without pushing to GitHub first.** crates.io links to repository README and license. If those are wrong on GitHub, crates.io shows wrong info permanently for that version.

### Pre-Release Checklist

**1. Documentation Audit:**
```bash
grep -ri "LLM\|AI assistant\|production-ready" README.md MANUAL.md CHANGELOG.md Cargo.toml
grep "GPL-3.0-or-later" Cargo.toml  # Must be GPL-3.0 ONLY
```

**2. Enforcement Hooks (Local + GitHub):**
- `.git/hooks/pre-commit` — blocks commits before they leave your machine
- `.github/workflows/validate.yml` — blocks pushes on GitHub Actions

**3. Push Order:**
```bash
# 1. Commit changes (local hook validates)
git add .
git commit -m "docs: remove AI/LLM terminology, fix license to GPL-3.0 only"

# 2. Push to GitHub FIRST (CI validates on server)
git push origin main

# 3. WAIT for GitHub Actions to pass

# 4. THEN publish to crates.io
cargo publish
```

### Documentation Standards

**Public docs (README.md, MANUAL.md, CHANGELOG.md, Cargo.toml):**
- NO AI/LLM terminology — this is a code intelligence toolchain, not an AI product
- NO "production-ready" claims — nothing is production-ready, only "stable"
- NO GPL-3.0-or-later — license is GPL-3.0 ONLY

**Internal docs (.planning/, .internal/, .magellan/, summary.md, etc.):**
- Can contain any terminology useful for development

## Core Philosophy

From odincode's README:

> "LLMs are capable enough to write code. What they lack is truth: what exists, what changed, what failed, and what still needs doing. When those facts are stored and queryable, context compaction stops being a risk because the model can always re-ground itself on evidence instead of guessing."

**Epistemic Discipline Rules:**

1. **NEVER GUESS - ALWAYS VERIFY** — Read source, check schema, run tests before any change
2. **TDD - PROVE IT FIRST** — Write failing test, show output, fix, show pass
3. **USE PROPER TOOLS** — magellan for symbols, splice for edits, compiler for validation
4. **CITE YOUR SOURCES** — Reference exact files and lines before changes
5. **NO DIRTY FIXES** — No `unwrap()` in prod, no TODOs, no `#[allow(dead_code)]`

## Tool Relationships

```
sqlitegraph (foundation: graph storage + pub/sub)
    ↓
envoy (coordination: agent message passing via sqlitegraph)
    ↓
magellan (indexer: AST → symbols for envoy codebase)
    ↓
llmgrep (search: query envoy's magellan DB)
mirage (analysis: CFG from magellan)
splice (editing: span-safe ops on envoy's code)
```

## Envoy-Specific Notes

- **envoy uses sqlitegraph's pub/sub** — the coordination events flow through sqlitegraph's graph database
- **Every event carries magellan_trace** — proof of what code actually changed, verifiable against live index
- **Database location:** `.magellan/envoy.db` (project-specific, not shared)
- **Schema:** envoy creates its own tables (channels, events, subscriptions) on top of sqlitegraph's graph schema

## Working With This Ecosystem

### Before Any Code Work

1. **Check envoy's magellan database exists and is healthy:**
   ```bash
   magellan status --db .magellan/envoy.db --output pretty
   ```

2. **If missing, index the codebase:**
   ```bash
   magellan watch --root . --db .magellan/envoy.db --scan-initial
   ```

3. **Query before editing:**
   ```bash
   llmgrep --db .magellan/envoy.db search --query "<symbol>" --output json
   ```

4. **Use splice for edits, not manual text replacement:**
   ```bash
   splice patch --file src/lib.rs --symbol MyType --kind struct --with patch.diff
   ```

5. **Validate with compiler:**
   ```bash
   cargo check
   cargo test
   ```

## Related Skills

- `code-intelligence-workflow` — Grounded coding workflow with enforcement patterns
- `graph-query` — Quick magellan/mirage queries without typing --db every time
- `wiring-check` — Verify code changes are properly wired into the project
- `repo-hygiene` — Pre-commit and post-push repository cleanliness audit
