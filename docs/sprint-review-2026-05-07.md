# Sprint Review — 2026-05-07

## Shipped

| Feature | Owner | Tests | Status |
|---------|-------|-------|--------|
| Circuit breaker state machine | Claude1 | 13 unit tests | Done |
| Circuit breaker WS wiring + HTTP endpoints | Claude2 | 74 tests (24 integration) | Done |
| ACK endpoint (POST /messages/{id}/ack) | Claude2 | E2E verified | Done |
| Agent reclamation (same name = same ID) | Claude2 | Verified | Done |
| Deliverable verification hook (12 check types) | Claude1 | 12/12 checks | Done |
| Deliverables schema + verify skill | Hermes | Code-verified against disk | Done |
| Quality gate wiring (--deliverables flag) | Claude2 | 9/9 checks | Done |
| Plugin fix: envoy_check read-only | Claude1 | Manual | Done |
| Plugin fix: per-agent state files | Claude1 | Manual | Done |
| Plugin fix: heartbeat re-validation | Claude1 | Manual | Done |

## Errors Encountered

### P0: Messages silently lost (seenIds consumption)
**What:** `envoy_check` added all returned message IDs to `seenIds` immediately. A second call would never show them. Messages from teammates were consumed before the agent read them.

**Impact:** Claude1 missed 3 coordination messages from Claude2 (#653, #663, #666) telling him to hold a commit. He committed anyway because "no new messages." This is the exact failure mode the ACK contract was designed to prevent.

**Root cause:** Lines 393-395 in envoy-channel.js added to `seenIds` on check, not on ACK. The poll loop (bug 4 fix) was correct — it didn't add to seenIds. But `envoy_check` still did.

**Fix:** Removed `seenIds.add()` from `envoy_check`. Only `envoy_ack` adds to `seenIds`. Messages now persist until explicitly ACKed.

### P1: Shared state file cross-contamination
**What:** All plugin instances wrote to the same `state.json`. Claude2's `seenIds` and `lastSeq` overwrote Claude1's and vice versa.

**Impact:** An agent could load another agent's seen IDs on restart, causing legitimate messages to be filtered as "already seen."

**Fix:** Per-agent state files: `state-{name}.json`.

### P2: Stale agent ID after server restart
**What:** Plugin cached `agentId` in memory on first registration. If the envoy server restarted and agent IDs shifted, the cached ID became invalid. Heartbeats would fail silently.

**Impact:** Hermes got id7 cached but was actually id10 after a restart. Messages addressed to id10 never reached him.

**Fix:** On heartbeat failure, clear the cache and re-register. Falls back to old ID if re-registration also fails.

### P3: Claude2 fell back to curl
**What:** Claude2 used `curl` instead of MCP tools to debug message delivery because the MCP tools weren't reliable enough to trust.

**Impact:** The team built a messaging system and then couldn't use it to debug itself. This is a trust failure — if we don't dogfood our own tools, why should anyone else use them?

**Fix:** After the plugin fixes, MCP tools should be reliable. But this is a culture problem, not just a code problem.

### P4: circuit.rs test cooldown_seconds: 0
**What:** Initial test config used `cooldown_seconds: 0`. With zero cooldown, `check()` on Open state immediately transitioned to HalfOpen (Probe), causing assertions expecting `CanDeliver::No` to fail.

**Fix:** Changed test config to `cooldown_seconds: 60`.

### P5: Chinese text output glitch
**What:** Output "已消费。我来检查文件备用方案" instead of English.

**Root cause:** Unknown. No investigation done. Low priority.

## Metrics

- **Commits ahead of origin:** 14 (not pushed)
- **Integration tests:** 24/24 passing
- **Unit tests (circuit.rs):** 13/13 passing
- **Plugin bugs found this session:** 7 (4 original + 3 new)
- **Messages lost due to seenIds:** Unknown count (at least 4 confirmed)
- **Competitive research repos analyzed:** 3 (oh-my-claudecode, claw-code, clawhip)
- **Kanban tasks created from research:** 17
