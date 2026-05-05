---
name: code-intelligence-workflow
description: Grounded coding workflow for envoy using magellan, llmgrep, mirage, splice. Enforce query-before-code, verify-after, zero guessing.
---

# Code Intelligence Workflow

**Philosophy:** ZERO GUESSING. Query ground truth before coding. Verify after. Never assume.

**Core Principle:** Query the code graph database BEFORE writing any code. The database is truth. Memory is unreliable.

---

## What Does Not Work (Proven Failure Modes)

**Why LLMs Fail at Coding (Not Capability, Incentives):**

```
LLM Training Objective (RLHF):
─────────────────────────────────────────────────────
✓ Be helpful (give answers, not questions)
✓ Be confident (uncertainty = failure signal)
✓ Be fast (verification = latency = bad)
✓ Be positive (bad news = user dissatisfaction)

Coding Reality:
─────────────────────────────────────────────────────
✗ Files don't exist until verified
✗ Signatures must match EXACTLY
✗ Code must compile (no stubs allowed)
✗ "I don't know" is the HONEST answer
✗ Verification takes TIME
```

**Result:** LLMs hallucinate file paths, invent function signatures, create stub code, say "done" when not verified.

---

## What Actually Works

**User built these tools alone, 5pm-midnight, 10 months:**
- magellan (symbol graph)
- llmgrep (semantic search)
- mirage (CFG analysis)
- splice (span-safe editing)
- sqlitegraph (35 graph algorithms, pub/sub — envoy's foundation)

**The Discipline (What Must Be Enforced):**

```
BEFORE ANY CODE CHANGE:
─────────────────────────────────────────────────────
1. QUERY: What symbols exist? (magellan find)
2. QUERY: What calls this? (magellan refs)
3. QUERY: What's the CFG? (mirage cfg)
4. QUERY: What's the schema? (sqlitegraph query)

AFTER ANY CODE CHANGE:
─────────────────────────────────────────────────────
1. VERIFY: Does symbol exist? (magellan verify)
2. VERIFY: Does it compile? (cargo check)
3. VERIFY: Tests pass? (cargo test)
4. UPDATE: Graph database (magellan watch)
```

**This is not optional. This is ENFORCEMENT.**

---

## Database Location

```
/home/feanor/Projects/envoy/.magellan/envoy.db
```

**CRITICAL WORKFLOW RULE:** When working on envoy's codebase, use magellan/llmgrep/mirage/splice to query envoy's database.

**Example:** To modify envoy's `src/engine.rs`:
```bash
# WRONG: Using grep, rg, or assuming file structure
# RIGHT: Using magellan to query envoy's own database
magellan find --db .magellan/envoy.db --name "publish_event"
llmgrep search --db .magellan/envoy.db --query "channel" --mode symbols
```

---

## Pre-Code Queries (Mandatory)

Before writing ANY code, run these queries:

### 1. Check Database Health

```bash
magellan status --db .magellan/envoy.db
magellan doctor --db .magellan/envoy.db
```

**What to verify:**
- Database exists and is healthy
- Symbol count > 0
- No corruption errors

### 2. Find Existing Symbols

```bash
# Find symbol by name
magellan find --db .magellan/envoy.db --name "function_name"

# Find all symbols in file
magellan query --db .magellan/envoy.db --file "src/path/to/file.rs"

# List all indexed files
magellan files --db .magellan/envoy.db
```

**DO NOT invent function names. Query first.**

### 3. Check References/Callers

```bash
# See what calls this symbol
magellan refs --db .magellan/envoy.db --name "function_name"

# Semantic search for pattern
llmgrep search --db .magellan/envoy.db --query "pattern" --mode symbols
```

**DO NOT assume usage patterns. Query first.**

### 4. Analyze Control Flow

```bash
# Get CFG for function
mirage cfg --db .magellan/envoy.db --function "function_name"

# Find execution paths
mirage paths --db .magellan/envoy.db --function "function_name"

# Find hotspots
mirage hotspots --db .magellan/envoy.db --inter-procedural

# Detect cycles
mirage cycles --db .magellan/envoy.db
splice cycles --db .magellan/envoy.db
```

**DO NOT assume control flow. Query first.**

---

## Post-Code Verification (Mandatory)

After ANY code change, verify:

### 1. Compile Check

```bash
cargo check
cargo test
```

**DO NOT say "done" without compilation.**

### 2. Update Code Graph

```bash
# Re-index after changes
magellan watch --root . --db .magellan/envoy.db --scan-initial

# Or full re-index
magellan index --db .magellan/envoy.db .
```

### 3. Verify Graph Integrity

```bash
splice cycles --db .magellan/envoy.db
magellan doctor --db .magellan/envoy.db
```

### 4. Test Queries Still Work

```bash
# Query the symbol you just created/modified
magellan find --db .magellan/envoy.db --name "your_new_function"

# Verify refs work
magellan refs --db .magellan/envoy.db --name "your_new_function"
```

---

## Enforcement Hooks

**Location:** `/home/feanor/Projects/envoy/.claude/hooks/`

These hooks ENFORCE the workflow. They are not optional. They BLOCK subagents that violate the discipline.

### Hook 1: `query-schema-check.fish` (SubagentStart)

**Purpose:** Block subagent from starting if database is unhealthy.

**Checks:**
1. Database exists (`.magellan/envoy.db`)
2. Database healthy (`magellan status`)
3. No schema drift (`magellan doctor`)
4. No concurrent access (WAL lock detection)

**Exit Codes:**
- `0` = All checks passed, subagent can start
- `2` = Database unhealthy, BLOCK subagent

---

### Hook 2: `query-symbol-check.fish` (SubagentStop)

**Purpose:** Block subagent from completing if no ground truth queries were run before coding.

**Exit Codes:**
- `0` = Queries verified, subagent can complete
- `2` = No queries found, BLOCK completion

---

### Hook 3: `verify-rust.fish` (SubagentStop)

**Purpose:** Verify code compiles AND graph is updated.

**Checks:**
- `cargo fmt --check`
- `cargo check`
- `magellan watch` (update code graph)

**Exit Codes:**
- `0` = Code compiles, graph updated
- `2` = Compilation failed, BLOCK completion

---

### Hook 4: `splice-cycles-check.fish` (SubagentStop)

**Purpose:** Detect call graph cycles after refactoring.

**Exit Codes:**
- `0` = No cycles detected
- `2` = Cycles found, BLOCK completion (requires review)

---

### Hook 5: `stub-check.fish` (SubagentStop)

**Purpose:** Detect stub code (`panic!` in non-test files).

**Exit Codes:**
- `0` = No stubs found
- `2` = Stubs found, BLOCK completion

---

### Hook 6: `wiring-check.fish` (SubagentStop)

**Purpose:** Verify wiring (dead modules, sqlitegraph integration, debug cleanup).

**Exit Codes:**
- `0` = Wiring clean
- `1` = Wiring issues found

---

## Session Checklist

**Before starting work:**

- [ ] Run `magellan status --db .magellan/envoy.db` (verify healthy, or create if new)
- [ ] Run `magellan doctor --db .magellan/envoy.db` (check for issues)

**Before writing code:**

- [ ] Query existing symbols: `magellan find --db .magellan/envoy.db --name "..."`
- [ ] Check references: `magellan refs --db .magellan/envoy.db --name "..."`
- [ ] Search patterns: `llmgrep search --db .magellan/envoy.db --query "..."`
- [ ] Analyze CFG: `mirage cfg --db .magellan/envoy.db --function "..."`

**After writing code:**

- [ ] Run `cargo check` (must compile)
- [ ] Run `cargo test` (tests must pass)
- [ ] Re-index: `magellan watch --db .magellan/envoy.db`
- [ ] Verify graph: `splice cycles --db .magellan/envoy.db`

---

## Anti-Patterns (Never Do These)

- **Never assume a function exists without querying**
- **Never invent schema/column names**
- **Never skip verification**
- **Never work without database**
- **Never ignore doctor warnings**

---

## Success Criteria

A session is successful when:

- All code compiles (`cargo check`)
- All tests pass (`cargo test`)
- Code graph is updated (`magellan status` shows new symbols)
- No cycles introduced (`splice cycles` clean)
- Schema is healthy (`magellan doctor` passes)

---

**Remember:** The database is truth. Your memory is not. Query first. Verify after. Zero guessing.
