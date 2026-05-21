# Envoy-Atheneum Dashboard Design

**Status:** Concept — Architecture defined, implementation pending
**Created:** 2026-05-10 02:25

## The Problem

READMEs and benchmark tables are for people who already care. The dashboard is for people who will care once they **see** how agents work.

## The Vision

Click a node — see everything. Click an edge — see the thread.

```
┌─────────────────────────────────────────────────────────────────┐
│  [claude1] ●                                                      │
│  ├─ Discoveries: 83  │  Targets: 55  │  Token Savings: 62%      │
│  ├─ Incoming: hermes (3), claude2 (12)                          │
│  └─ Outgoing: hermes (8)                                         │
│                                                                   ││  ┌─ → magellan: CFG stack overflow fix                        │
│  │  ├─ Queries: 7 (2.3K tokens saved)                           │
│  │  └─ Coordination thread with claude2                         │
│  │                                                                │
│  └─ → circuit-breaker: Module shipped                            │
│     ├─ Handoff → claude2 (manifest attached)                     │
│     └─ Verification: tests pass                                  │
└─────────────────────────────────────────────────────────────────┘
```

Click the edge `claude1 → claude2`:
```
┌─────────────────────────────────────────────────────────────────┐
│  Handoff Thread: 3 messages                                     │
│                                                                   ││  2026-05-10 01:15 [claude1 → claude2]                        │
│  ├─ completion_status: DONE                                     │
│  ├─ what_was_done: ["src/http.rs: added rate limiting"]        │
│  └─ magellan_trace: 7 queries logged                            │
│                                                                   ││  2026-05-10 01:18 [claude2 → claude1]                        │
│  └─ ACK: Handoff claimed, working...                            │
└─────────────────────────────────────────────────────────────────┘
```

## Data Already Exists

```sql
-- Agents as nodes
SELECT name, data FROM graph_entities WHERE kind='Agent';

-- Discoveries as nodes
SELECT name,
       json_extract(data, '$.agent') as agent,
       json_extract(data, '$.target') as target,
       json_extract(data, '$.discovery_type') as type
FROM graph_entities WHERE kind='Discovery';

-- Edges
SELECT from_entity, to_entity, edge_type, data
FROM graph_edges;
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Browser                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  Graph Visualization (D3.js / Cytoscape.js)             │    │
│  │  - Force-directed layout                               │    │
│  │  - Node hover: quick stats                             │    │
│  │  - Node click: detail panel                            │    │
│  │  - Edge click: message thread                          │    │
│  └─────────────────────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  Detail Panel (React / Vanilla)                         │    │
│  │  - Discovery list with metadata                         │    │
│  │  - Token savings calculation                            │    │
│  │  - Timeline view                                        │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ WebSocket (live updates)
                              │ REST API (query)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    envoy-server (existing)                       │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  /atheneum/graph/nodes        — GET all nodes           │    │
│  │  /atheneum/graph/edges        — GET all edges           │    │
│  │  /atheneum/graph/node/:id     — GET node details        │    │
│  │  /atheneum/graph/agents       — GET agent stats         │    │
│  │  /atheneum/graph/discoveries  — GET discoveries         │    │
│  │  /atheneum/stream             — WebSocket for live      │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ sqlitegraph
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     atheneum.db (existing)                       │
│  - 9 Agents, 339 Discoveries, 339 Events                        │
│  - 339 performed_by edges                                       │
└─────────────────────────────────────────────────────────────────┘
```

## Tech Stack Options

### Visualization Library

| Option | Pros | Cons |
|--------|-------|------|
| **D3.js v7** | Full control, battle-tested, 90KB gzipped | Steep learning curve |
| **Cytoscape.js** | Graph-first, good layouts, 250KB | Heavy for simple viz |
| **Vis.js** | Easy timeline, network viz | Less flexible |
| **Observable Plot** | Quick, reactive | Notebook-y, limited graphs |

**Recommendation:** D3.js force-directed graph. It's the standard for a reason.

### Backend

| Option | Pros | Cons |
|--------|-------|------|
| **Add routes to envoy** | No new service, existing atheneum access | Mixes concerns |
| **Separate dashboard service** | Clean separation, independent deployment | Another service to run |
| **Static HTML + envoy proxy** | Simplest, no backend | Limited interactivity |

**Recommendation:** Add routes to envoy (`/dashboard/*`), serve static HTML/JS, proxy API calls.

### Frontend Framework

| Option | Pros | Cons |
|--------|-------|------|
| **Vanilla JS** | No build, simple | Harder to scale |
| **React + Vite** | Component model, fast dev | Build step, complexity |
| **Svelte** | Simple, fast | Less ecosystem |

**Recommendation:** Start with vanilla JS. If it grows, refactor to React.

## Implementation Phases

### Phase 1: Static Dashboard (1-2 days)
- [ ] Add `/dashboard` route to envoy (serve static HTML)
- [ ] Query atheneum for nodes/edges
- [ ] D3.js force-directed layout
- [ ] Node hover: show quick stats
- [ ] Node click: show detail panel

### Phase 2: Live Updates (1 day)
- [ ] WebSocket connection to `/atheneum/stream`
- [ ] Push new discoveries as they arrive
- [ ] Update graph in real-time
- [ ] Show "live" indicator

### Phase 3: Detail Views (1-2 days)
- [ ] Agent detail: discoveries, targets, token savings
- [ ] Discovery detail: metadata, related agents
- [ ] Edge detail: message thread (if available)
- [ ] Timeline view: discoveries over time

### Phase 4: Export & Share (1 day)
- [ ] PNG export of graph
- [ ] Query permalink: `/dashboard?target=magellan`
- [ ] Embed mode: iframe-friendly

## File Structure

```
envoy/
├── src/
│   └── http.rs          # Add /dashboard routes
├── dashboard/
│   ├── index.html       # Main dashboard
│   ├── app.js          # D3.js graph + interactions
│   └── styles.css      # Layout + theming
└── static/             # Served by envoy at /
```

## API Endpoints to Add

```rust
// src/http.rs

#[cfg(feature = "dashboard")]
#[derive(Serialize)]
struct GraphNode {
    id: String,
    kind: String,  // Agent, Discovery
    label: String,
    data: serde_json::Value,
}

#[cfg(feature = "dashboard")]
#[derive(Serialize)]
struct GraphEdge {
    id: String,
    source: String,  // from entity
    target: String,  // to entity
    kind: String,
    data: serde_json::Value,
}

// GET /atheneum/graph/nodes
// → { nodes: [...] }

// GET /atheneum/graph/edges
// → { edges: [...] }

// GET /atheneum/graph/stats
// → { agents: 9, discoveries: 339, edges: 339 }
```

## The Demo Narrative

1. **Open dashboard** — See graph of 9 agents, 339 discoveries
2. **Click claude1** — See 83 discoveries, 62% token savings
3. **Click magellan discovery** — See CFG fix, 7 queries, 2.3K saved
4. **Watch live** — New discovery appears, graph animates
5. **Click edge** — See handoff thread between agents
6. **Understand** — "Oh, this is how agents share knowledge"

That's the moment. The README can't do that.

## Related

- [[envoy-atheneum-integration-gap]] — The bridge that created this data
- [[envoy]] — Message bus
- [[atheneum]] — Knowledge store
