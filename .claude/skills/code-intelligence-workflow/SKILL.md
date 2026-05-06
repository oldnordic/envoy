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
# CRITICAL: Always use --root ./src (NOT --root .). Indexing the project root
# includes Cargo.toml, .git/, target/, .magellan/ — polluting the symbol graph
# with non-code noise.
magellan watch --root ./src --db .magellan/envoy.db --scan-initial

# Or full re-index
magellan index --db .magellan/envoy.db ./src
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

## CLI Command Reference (Complete)

### magellan (symbol graph)

| Command | Purpose |
|---------|---------|
| `magellan watch --root ./src --db <db>` | Watch & index source files |
| `magellan index --db <db> <path>` | One-shot full index |
| `magellan status --db <db>` | Database health & stats |
| `magellan doctor --db <db>` | Diagnose schema/consistency issues |
| `magellan find --db <db> --name "sym"` | Find symbol by name (substring) |
| `magellan find --db <db> --list-glob "pattern"` | List symbols matching glob |
| `magellan get --db <db> --name "sym"` | Get symbol details by name |
| `magellan get-file --db <db> --file "path"` | Get file-level metadata |
| `magellan query --db <db> --file "path"` | List symbols in a file |
| `magellan refs --db <db> --name "sym"` | Show callers/callees |
| `magellan files --db <db>` | List all indexed files |
| `magellan dead-code --db <db> --entry "main"` | Dead code from entry point |
| `magellan chunks --db <db> --file "path"` | Get file as semantically chunked spans |
| `magellan chunk-by-span --db <db> --file "path" --start N --end N` | Get chunk at specific span |
| `magellan chunk-by-symbol --db <db> --name "sym"` | Get chunk for a symbol |
| `magellan label --db <db> --name "sym" --label "L"` | Add label to symbol |
| `magellan collisions --db <db>` | Detect symbol name collisions |
| `magellan ast --db <db> --file "path"` | Dump AST nodes for file |
| `magellan find-ast --db <db> --name "sym"` | Find AST node for symbol |
| `magellan condense --db <db>` | Condense/compact the database |
| `magellan paths --db <db> --from "a" --to "b"` | Find call paths between symbols |
| `magellan slice --db <db> --name "sym"` | Program slice for symbol |
| `magellan context build --db <db> --query "Q"` | Build LLM context from query |
| `magellan context summary --db <db>` | Summarize current context |
| `magellan context file --db <db> --file "path"` | Context for a specific file |
| `magellan migrate --db <db>` | Run database schema migrations |
| `magellan migrate-backend --db <db> --to <backend>` | Migrate between backends |
| `magellan verify --db <db> --name "sym"` | Verify symbol exists in graph |
| `magellan refresh --db <db>` | Refresh/re-index dirty files |
| `magellan registry scan --db <db>` | Scan project for new files |
| `magellan registry list --db <db>` | List indexed file registry |

### llmgrep (semantic search)

| Command | Purpose |
|---------|---------|
| `llmgrep search --db <db> --query "Q" --mode symbols` | Semantic symbol search |
| `llmgrep search --db <db> --query "Q" --mode references` | Semantic reference search |
| `llmgrep ast --db <db> --file "path"` | Dump AST for file |
| `llmgrep find-ast --db <db> --name "sym"` | Find AST node for symbol |
| `llmgrep complete --db <db> --prefix "name"` | Symbol name completion |
| `llmgrep lookup --db <db> --name "sym"` | Lookup symbol details |

### mirage (CFG analysis)

| Command | Purpose |
|---------|---------|
| `mirage status --db <db>` | Database health |
| `mirage cfg --db <db> --function "name"` | Control flow graph for function |
| `mirage paths --db <db> --function "name"` | Execution paths |
| `mirage hotspots --db <db> [--inter-procedural]` | Cyclomatic complexity hotspots |
| `mirage cycles --db <db>` | Detect call graph cycles |
| `mirage dominators --db <db> --function "name"` | Dominator tree |
| `mirage loops --db <db> --function "name"` | Natural loop detection |
| `mirage unreachable --db <db> --function "name"` | Unreachable code detection |
| `mirage patterns --db <db> --function "name"` | CFG pattern recognition |
| `mirage frontiers --db <db> --function "name"` | Dominance frontier analysis |
| `mirage verify --db <db>` | Verify CFG integrity |
| `mirage blast-zone --db <db> --function "name"` | Impact analysis for changes |
| `mirage slice --db <db> --function "name" --criterion "line"` | Program slicing |
| `mirage hotpaths --db <db> --entry "main"` | Hot path identification |
| `mirage diff --db <db> --function "a" --function "b"` | CFG diff between functions |
| `mirage icfg --db <db> --function "name"` | Inter-procedural CFG |
| `mirage coverage --db <db>` | Code coverage analysis |
| `mirage migrate --db <db>` | Run database migrations |

### splice (span-safe editing)

| Command | Purpose |
|---------|---------|
| `splice status --db <db>` | Database health |
| `splice plan --db <db> rename --name "old" --new-name "new"` | Plan a rename |
| `splice rename --db <db> --name "old" --new-name "new"` | Execute rename |
| `splice patch --db <db> --file "path" --old "code" --new "code"` | Span-safe patch |
| `splice create --db <db> --file "path" --content "..."` | Create new file |
| `splice delete --db <db> --name "sym"` | Delete symbol |
| `splice batch --db <db> operations.yaml` | Batch operations |
| `splice apply-files --db <db> patches.yaml` | Apply file-level patches |
| `splice snapshots list/show/restore` | Snapshot management |
| `splice verify --db <db>` | Verify splice integrity |
| `splice log --db <db>` | Operation history log |
| `splice explain --db <db> --operation-id <id>` | Explain an operation |
| `splice search --db <db> --query "Q"` | Search operations |
| `splice get --db <db> --name "sym"` | Get symbol details |
| `splice export --db <db> --format json` | Export graph data |
| `splice cycles --db <db>` | Call graph cycle detection |
| `splice dead-code --db <db> --entry "main"` | Dead code detection |
| `splice refs --db <db> --name "sym" --path "file"` | Symbol references |
| `splice validate-proof --db <db> --operation-id <id>` | Validate edit proof |
| `splice migrate-db --db <db>` | Migrate splice database |
| `splice undo --db <db> --operation-id <id>` | Undo a splice operation |

---

## AI-Generated Rust Anti-Patterns

LLMs consistently produce these anti-patterns in Rust. Recognize and reject them.

### 1. Clone Escape Hatch

```rust
// ANTI-PATTERN: Slapping .clone() to fix borrow checker errors
fn process(data: &Data, config: &Config) -> Result<Output> {
    let cloned = data.clone();  // <- avoids thinking about ownership
    let result = compute(cloned, config.clone());
    Ok(result)
}
```

**Why it's wrong:** Each `.clone()` hides a design issue. The real fix is restructuring ownership, using references, or returning the value.

**Clippy lints:** `clippy::clone_on_copy`, `clippy::redundant_clone`, `clippy::clone_on_ref_ptr`

**Correct approach:** Restructure so data flows without cloning. Use `Cow<str>`, `Arc<T>`, or redesign function signatures to pass ownership where needed.

### 2. Giant Match Blocks

```rust
// ANTI-PATTERN: Match on string/enum with 20+ arms, each duplicating logic
fn handle_command(cmd: &str, ctx: &mut Context) -> Result<()> {
    match cmd {
        "create" => { ctx.init(); ctx.validate(); ctx.persist(); }
        "update" => { ctx.init(); ctx.validate(); ctx.persist(); }
        "delete" => { ctx.init(); ctx.validate(); ctx.remove(); }
        // ... 30 more arms
        _ => return Err(Error::Unknown),
    }
}
```

**Why it's wrong:** Copy-paste logic in each arm, impossible to maintain, O(n) matching on strings.

**Clippy lints:** `clippy::single_match_else`, `clippy::match_same_arms`

**Correct approach:** Use a trait object / strategy pattern, or at minimum deduplicate with helper functions and use `enum` instead of `&str`.

### 3. Overconstrained Lifetimes

```rust
// ANTI-PATTERN: Adding lifetime annotations everywhere without understanding
fn process<'a, 'b, 'c>(input: &'a str, config: &'b Config, cache: &'c Cache) -> &'a str
where
    'b: 'a,
    'c: 'a,
{
    // ...
}
```

**Why it's wrong:** Unnecessary lifetime constraints make the function impossible to call in many contexts. LLMs add them "to be safe."

**Clippy lints:** `clippy::needless_lifetimes`, `clippy::lifetime_pass_by_value`

**Correct approach:** Use lifetime elision rules. Only add explicit lifetimes when elision fails. If you need `'static`, return an owned type.

### 4. unwrap() as Strategy

```rust
// ANTI-PATTERN: Using unwrap/expect as default error handling
fn read_config(path: &str) -> Config {
    let content = std::fs::read_to_string(path).unwrap(); // crashes on missing file
    let config: Config = serde_json::from_str(&content).expect("valid JSON"); // crashes on bad input
    config
}
```

**Why it's wrong:** Panics in library/server code are unrecoverable. `unwrap()` is acceptable ONLY in tests or when failure is provably impossible.

**Clippy lints:** `clippy::unwrap_used`, `clippy::expect_used`

**Correct approach:** Return `Result<T, E>`, use `?` operator, provide meaningful error context with `.context()` or `.map_err()`.

### Quick Reference: Clippy Flags for AI Code Review

```bash
# Run all recommended clippy checks
cargo clippy --all-targets -- -D warnings -W clippy::unwrap_used -W clippy::expect_used -W clippy::clone_on_copy -W clippy::redundant_clone

# Specifically catch AI anti-patterns
cargo clippy -- -W clippy::needless_lifetimes -W clippy::match_same_arms -W clippy::single_match_else
```

**Rule:** If `cargo clippy` warns about any of these, the LLM wrote bad Rust. Fix it.

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
