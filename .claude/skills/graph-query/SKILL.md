---
name: graph-query
description: Run common magellan graph queries against the envoy project database without typing the full --db path every time.
---

# graph-query

Execute common magellan and mirage queries against `.magellan/envoy.db`.

## Arguments

```
/graph-query <command> [args...]
```

## Supported Commands

### Graph Queries

| Command | Description | Example |
|---------|-------------|---------|
| `find` | Find symbol by name | `/graph-query find publish_event` |
| `refs` | Show references to/from symbol | `/graph-query refs publish_event --direction in` |
| `query` | List symbols in a file | `/graph-query query src/engine.rs` |
| `get` | Get symbol details by ID | `/graph-query get <ID>` |
| `get-file` | Get all symbols in a file | `/graph-query get-file src/engine.rs` |
| `files` | List indexed files | `/graph-query files` |

### Call Graph & Algorithms

| Command | Description | Example |
|---------|-------------|---------|
| `cycles` | Find call graph cycles | `/graph-query cycles` |
| `reachable` | Show reachable symbols from entry | `/graph-query reachable <ID>` |
| `dead-code` | Find dead code from entry | `/graph-query dead-code <ID>` |
| `paths` | Enumerate all execution paths | `/graph-query paths <func>` |
| `slice` | Program slicing (forward/backward impact) | `/graph-query slice <func>` |
| `condense` | Analyze condensation graph (SCCs to DAG) | `/graph-query condense` |
| `hotspots` | Find high-risk functions | `/graph-query hotspots` |

### AST & Chunks

| Command | Description | Example |
|---------|-------------|---------|
| `ast` | Show AST for a file or span | `/graph-query ast src/engine.rs` |
| `find-ast` | Find AST nodes by kind | `/graph-query find-ast Function` |
| `chunks` | List code chunks for a file | `/graph-query chunks src/engine.rs` |
| `chunk-by-span` | Get chunk by line/byte span | `/graph-query chunk-by-span src/engine.rs --start 10 --end 50` |
| `chunk-by-symbol` | Get chunk for a symbol | `/graph-query chunk-by-symbol publish_event` |

### Labels & Collisions

| Command | Description | Example |
|---------|-------------|---------|
| `label` | Query symbols by label | `/graph-query label fn --path src/` |
| `collisions` | Find name collisions | `/graph-query collisions` |

### Context (LLM)

| Command | Description | Example |
|---------|-------------|---------|
| `context build` | Build LLM context for a query | `/graph-query context build --query "how does X work"` |
| `context summary` | Summarize indexed codebase | `/graph-query context summary` |
| `context list` | List symbols (paginated) | `/graph-query context list --kind fn --page 1` |
| `context symbol` | Get context for a specific symbol | `/graph-query context symbol publish_event` |
| `context file` | Get context for a file | `/graph-query context file src/engine.rs` |
| `context impact` | Show impact analysis for a change | `/graph-query context impact <func>` |
| `context affected` | Show what would be affected | `/graph-query context affected <func>` |

### Maintenance

| Command | Description | Example |
|---------|-------------|---------|
| `status` | Show database statistics | `/graph-query status` |
| `doctor` | Check database health | `/graph-query doctor` |
| `refresh` | Re-index changed files | `/graph-query refresh` |
| `verify` | Verify database integrity | `/graph-query verify` |
| `migrate` | Migrate database schema | `/graph-query migrate` |
| `registry scan` | Scan for registered databases | `/graph-query registry scan` |
| `registry list` | List registered databases | `/graph-query registry list` |

### Mirage (CFG Analysis)

| Command | Description | Example |
|---------|-------------|---------|
| `cfg` | Show CFG for function | `/graph-query cfg publish_event` |

## Database Path

All commands automatically use `--db .magellan/envoy.db`.

## Examples

```bash
# Find a symbol
/graph-query find "publish_event"

# Show incoming references
/graph-query refs "publish_event" --direction in

# Show outgoing references
/graph-query refs "publish_event" --direction out

# Get symbol details by ID
/graph-query get <symbol_id>

# Get AST for a file
/graph-query ast src/engine.rs

# Get chunks for context windowing
/graph-query chunks src/engine.rs

# Build LLM context
/graph-query context build --query "how does event routing work"
/graph-query context summary

# Get CFG with 4D coordinates
/graph-query cfg "publish_event"

# Find hotspots (high complexity functions)
/graph-query hotspots

# Check database health
/graph-query doctor
/graph-query verify
```

## AI Anti-Patterns

- NEVER guess symbol names or file paths — always query first with `find` or `files`
- NEVER assume a function exists without `find` evidence
- NEVER skip `--preview` when using splice edits — always verify before applying
- NEVER claim code structure without running a query — "I think" is not evidence
- NEVER modify code without checking `refs` first — know what calls the symbol
- NEVER forget to re-index after edits (`magellan refresh` or `magellan watch`)
- NEVER trust cached memory of the codebase — the database is truth, memory is not
