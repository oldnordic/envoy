# Dashboard Implementation Plan — Grounded Coding Analysis

**Created:** 2026-05-10 02:30
**Status:** Gate 1 Complete ✅ — Backend API implemented
**Updated:** 2026-05-10 03:00

## Progress

- ✅ **Phase 0:** Analysis complete
- ✅ **Gate 1:** Backend API (`src/dashboard.rs`, 287 lines, 5 handlers)
- ✅ **TDD Tests:** 5 tests passing, 123 total tests
- ⏳ **Gate 2:** HTTP routes (pending)
- ⏳ **Gate 3:** Frontend D3.js visualization (pending)
- ⏳ **Gate 4:** WebSocket live updates (pending)

## Evidence Gathering (Phase 0 Layer 1-4)

### Existing Code Structure

**HTTP Routes (`src/http.rs:482-530`):**
- `/agents`, `/messages`, `/events`, `/audit`, `/tasks/*`
- Atheneum routes cfg-gated (discoveries, handoffs, knowledge)

**AppState (`src/http.rs:76-94`):**
- agent_registry, audit_store, task_store, message_store
- ws_registry (for live updates)
- atheneum_path (cfg-gated)

**Database Schema (sqlitegraph):**
- graph_entities: id, kind, name, file_path, data
- graph_edges: id, from_id, to_id, edge_type, data

**Data Snapshot (from atheneum.db):**
- 9 Agents, 339 Discoveries, 339 Events
- 339 `performed_by` edges (Agent → Discovery)

**Discovery Types:**
- hook_result: 139, coordination: 133, code_review: 48
- git_operation: 13, response: 5, documentation: 1

## Design — Four Viewports

1. **Graph View** — D3.js force-directed graph (agents, discoveries, edges)
2. **Kanban View** — Tasks grouped by TODO/IN_PROGRESS/DONE
3. **Audit View** — Timeline of events and actions
4. **Live View** — WebSocket updates for real-time activity

## Implementation Plan

### Phase 1: Backend API (New HTTP Endpoints)

**File:** `src/dashboard.rs` (NEW)

**Structs:**
```rust
#[derive(Serialize)]
struct GraphNode {
    id: String,
    kind: String,      // "Agent" | "Discovery" | "Event"
    label: String,
    data: serde_json::Value,
}

#[derive(Serialize)]
struct GraphEdge {
    id: String,
    source: String,
    target: String,
    kind: String,
    data: serde_json::Value,
}

#[derive(Serialize)]
struct DashboardStats {
    agents: usize,
    discoveries: usize,
    edges: usize,
    discovery_types: HashMap<String, usize>,
}
```

**Endpoints:**
- `GET /api/dashboard/graph/nodes` — All graph nodes
- `GET /api/dashboard/graph/edges` — All graph edges
- `GET /api/dashboard/graph/stats` — Summary statistics
- `GET /api/dashboard/tasks` — Tasks grouped by state (kanban)
- `GET /api/dashboard/audit` — Audit timeline
- `WS /api/dashboard/stream` — WebSocket for live updates

### Phase 2: Frontend Structure

**Directory:**
```
dashboard/
├── index.html       # Main entry, view switching
├── app.js           # Main logic, WebSocket
├── graph.js         # D3.js force-directed graph
├── kanban.js        # Drag-drop kanban board
├── audit.js         # Timeline view
└── styles.css       # Dark theme styling
```

**No build step** — vanilla JS + CDN for D3.js

### Phase 3: Graph Visualization

**D3.js force-directed layout:**
- Nodes: Agents (green, larger), Discoveries (blue, smaller)
- Edges: performed_by relationships
- Interactions: hover tooltip, click detail panel
- Physics: charge, collision detection, link distance

### Phase 4: Kanban View

**Columns:** TODO, IN_PROGRESS, DONE
- Task cards with id, description, assignee
- Drag-drop between columns (optional Phase 4)
- Click card → detail modal

### Phase 5: Live Updates

**WebSocket message types:**
- `new_discovery` — Add node to graph
- `task_update` — Move card between columns
- `audit_event` — Add entry to timeline
- `agent_status` — Update online/offline indicator

## Test Strategy (Gate 1)

**tests/dashboard_api_tests.rs:**
- `test_graph_nodes_returns_valid_structure`
- `test_graph_edges_link_valid_nodes`
- `test_graph_stats_matches_db`
- `test_kanban_groups_tasks_by_state`
- `test_audit_timeline_chronological`

## Verification (Gate 3)

```bash
cargo build --release --features "atheneum,dashboard"
cargo test --features "atheneum,dashboard"
systemctl --user restart envoy
curl http://localhost:9876/api/dashboard/graph/stats | jq .
xdg-open http://localhost:9876/dashboard/
```

## Files to Create/Modify

| File | Action | Lines |
|------|--------|-------|
| `src/dashboard.rs` | Create | ~300 |
| `src/http.rs` | Modify (add routes) | +50 |
| `src/lib.rs` | Modify (mod dashboard) | +5 |
| `dashboard/*` | Create (5 files) | ~580 |
| `tests/dashboard_api_tests.rs` | Create | ~200 |

**Total:** ~1,135 lines

## Dependencies

**Cargo.toml:**
```toml
[features]
dashboard = ["tower-http/fs"]

[dependencies]
tower-http = { version = "0.5", optional = true }
```

**HTML (CDN):**
```html
<script src="https://cdn.jsdelivr.net/npm/d3@7"></script>
```

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| D3.js learning curve | Start simple, use examples |
| Large graph performance | Lazy loading, pagination |
| WebSocket disconnect | Auto-reconnect with backoff |
| XSS in innerHTML | Use textContent, sanitize input |

## Next Steps

**Gate 0 → Gate 1:**
1. Create `src/dashboard.rs` with structs
2. Write TDD tests (watch them fail)
3. Implement handlers (watch them pass)
4. Create `dashboard/` with HTML skeleton
5. Add D3.js with mock data, then real API
6. Wire WebSocket for live updates
7. Verify end-to-end

---

**Ready for Gate 0 approval.**
**Estimated: 6-8 hours full, 2-3 hours backend only.**
