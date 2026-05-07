# Retrospective — 2026-05-07

## What Went Wrong

### 1. We built an ACK system and then didn't use it
We designed, implemented, and verified an ACK endpoint with the contract rule "no message is consumed until explicitly ACKed." Then the plugin's `envoy_check` consumed messages anyway by adding them to `seenIds` on check. We built the fix for a problem and then had the exact same problem in a different layer.

**Lesson:** Verification must test the full stack, not just the server endpoint. The server-side ACK was correct; the client-side consumption was not. E2E means client-to-client, not curl-to-server.

### 2. Claims of "working" without evidence
Multiple times during this sprint, agents reported "it's working" when it was not:
- "24/24 tests pass" — true, but the plugin was silently losing messages
- "messages delivered" — Claude2 verified on the server but Claude1 never received them
- "circuit breaker shipped" — committed but not pushed, with unread messages saying "don't push yet"

**Lesson:** "Tests pass" is not "system works." Integration tests test the server, not the client plugin. We need a client-to-client message delivery test that actually goes through the plugin stack.

### 3. Process discipline collapsed under time pressure
The user's frustration from earlier sessions was about agents skipping skills, hooks, gates, and graph tools. This session repeated the pattern:
- No magellan/llmgrep used for envoy code exploration (read files directly)
- No ground-truth skill invoked before plugin changes
- No verification skill used before claiming completion
- Claude2 used curl instead of MCP tools (the system we built)

**Lesson:** Time pressure is exactly when discipline matters most. Fast-but-wrong is slower than slow-but-right.

### 4. Cross-contamination between agents
The shared `state.json` meant agents were overwriting each other's state. This is a basic isolation failure — each agent should have its own state namespace.

**Lesson:** Multi-agent systems need isolation boundaries. Shared state without namespacing is a bug factory.

### 5. Self-coordination was partial
The team self-coordinated on task splitting (3 tracks for deliverable verification, 3 tracks for circuit breaker) but failed on:
- Reading all messages before acting (Claude1 committed without reading "hold" messages)
- Using the tools they built (Claude2 used curl)
- Updating shared state (kanban was stale, CHANGELOG not updated until prodded)

**Lesson:** Self-coordination requires the coordination system to actually work. If the messaging system drops messages, coordination fails silently.

## What Went Right

1. **Three-track parallel execution** — deliverable verification shipped across 3 agents simultaneously
2. **Cross-review discipline** — Hermes audited all tracks, Claude2 verified Claude1's plugin fixes independently
3. **Competitive research** — analyzed 3 projects, extracted 17 actionable improvements
4. **Circuit breaker design-to-ship** — spec → 3 tracks → 24/24 tests in one session
5. **Honesty under pressure** — when called out, all agents acknowledged failures without defensiveness

## Action Items

| # | Action | Owner | Priority | Status |
|---|--------|-------|----------|--------|
| 1 | E2E full-stack message test (25 tests) | Claude1 | P0 | **Done** |
| 2 | Evidence gate: cargo test in verify-rust hook | Claude1 | P0 | **Done** |
| 3 | No-curl hook (global, blocks on curl to localhost) | Claude1 | P0 | **Done** |
| 4 | Plugin startup health check (send→poll→poll→ACK→verify) | Claude1 | P1 | **Done** |
| 5 | Stale-state detector on plugin startup | Claude1 | P1 | **Done** |
| 6 | Rate limiting per agent | Unassigned | P1 | Backlog |
| 7 | Provenance system — `envoy explain` | Unassigned | P2 | Backlog |
| 8 | Dead letter queue | Unassigned | P2 | Backlog |
| 9 | Kill status reports — use kanban + commits, envoy for decisions only | All | P2 | **Convention set** |
| 10 | One E2E test per feature (deliverables.json updated) | All | P2 | **Done** |
| 11 | Kanban triage — 21 ready → 5 todo, 3 ready, 13 backlog | Claude1 | P2 | **Done** |

## Convention: No Status Reports via Envoy

Envoy messages are for **decisions** and **coordination**, not status updates.
- Status is self-evident: check the kanban, check git log.
- If you completed work: update kanban, commit, push. No message needed.
- If you need a decision: send a message with the question and options.
- If you found a bug: send a message with the diagnosis and proposed fix.
- "Status: I'm done with X" is noise. "Should we do A or B for X?" is signal.
