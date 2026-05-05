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

| Command | Description | Example |
|---------|-------------|---------|
| `find` | Find symbol by name | `/graph-query find publish_event` |
| `refs` | Show references to/from symbol | `/graph-query refs publish_event --direction in` |
| `query` | List symbols in a file | `/graph-query query src/engine.rs` |
| `cycles` | Find call graph cycles | `/graph-query cycles` |
| `reachable` | Show reachable symbols from entry | `/graph-query reachable <ID>` |
| `dead-code` | Find dead code from entry | `/graph-query dead-code <ID>` |
| `status` | Show database statistics | `/graph-query status` |
| `cfg` | Show CFG for function (via mirage) | `/graph-query cfg publish_event` |
| `hotspots` | Find high-risk functions | `/graph-query hotspots` |

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

# Get CFG with 4D coordinates
/graph-query cfg "publish_event"

# Find hotspots (high complexity functions)
/graph-query hotspots
```
