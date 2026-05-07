# Post-Mortem: seenIds Silent Message Loss

**Date:** 2026-05-07
**Severity:** P0 — coordination messages silently dropped
**Duration:** Unknown (hours — from plugin deployment to fix)
**Impact:** At least 4 messages lost. Claude1 committed without reading hold messages.

---

## Timeline

1. **Session start** — Plugin loaded, `state.json` read from disk. `seenIds` populated from previous session.
2. **Poll loop runs** — Messages fetched via `GET /messages?to=id&since=lastSeq`. Channel notifications pushed. `lastSeq` advanced. `seenIds` NOT touched (bug 4 fix was correct here).
3. **Agent calls `envoy_check`** — Messages fetched via `GET /messages?to=id&since=0`. Filtered by `!seenIds.has(m.message_id)`. **All returned messages added to `seenIds`** (lines 393-395).
4. **Agent reads response** — Sees messages, processes some, conversation continues.
5. **Context compaction** — Messages in conversation context are summarized. Agent loses access to original message content.
6. **Agent calls `envoy_check` again** — All previously returned messages are in `seenIds`. Returns 0 new messages. Agent reports "no new messages."
7. **Claude2 sends messages #653, #663, #666** — Messages stored on server, channel notifications pushed.
8. **Claude1's poll loop advances `lastSeq` past those messages** — Channel notification rendered in conversation.
9. **Claude1 calls `envoy_check`** — Messages NOT in `seenIds` yet... BUT: the poll loop already advanced `lastSeq` in state. If `envoy_check` is called and the state file was saved by another agent's instance (shared state file), the `seenIds` could already contain those IDs. OR: `envoy_check` returns them once, adds to `seenIds`, and they're gone on the next call.
10. **Claude1 reports "0 new messages"** — Commits circuit breaker. Claude2's "don't push" messages unread.

## Root Cause

The `envoy_check` handler added message IDs to `seenIds` immediately upon returning them to the agent. This violated the ACK contract: "no message is consumed until explicitly ACKed." The contract existed on the server side (ACK endpoint) but not on the client side (plugin).

Additionally, the shared `state.json` file meant that one agent's `seenIds` could be loaded by another agent's instance on restart.

## Why It Wasn't Caught

1. **No client-to-client test.** Integration tests verify server endpoints, not the plugin's message delivery pipeline.
2. **The poll loop fix (bug 4) was correct** — it stopped adding to `seenIds`. But the same bug still existed in `envoy_check`.
3. **Messages appeared to work** — channel notifications rendered in the terminal, creating the illusion of delivery. The agent saw the notification text but couldn't access the full message.

## Fix

1. **`envoy_check` is now read-only** — it never adds to `seenIds`. Only `envoy_ack` does.
2. **Per-agent state files** — `state-{name}.json` prevents cross-contamination.
3. **Messages persist until ACKed** — an agent will see the same messages on every `envoy_check` call until they explicitly ACK them.

## Clarification: P2 Stale Agent ID

The original report stated Hermes cached id7 and got id10 after restart. In reality: Hermes's Python plugin loads fresh per session (no persistent agentId cache). The id10 assignment was a server-side artifact from the re-registration after restart. The JS plugin (used by Claude1 and Claude2) does have the in-memory cache problem; the Python plugin (used by Hermes) does not.

## Lessons

1. **Test the full stack.** Server tests don't catch client bugs.
2. **Contracts must be enforced end-to-end.** The ACK contract on the server was undermined by the plugin's consumption on check.
3. **"It works" requires evidence at every layer.** "Server received the message" ≠ "Agent read the message."
4. **Shared state without namespacing is always a bug.** Multi-agent systems need isolation.
