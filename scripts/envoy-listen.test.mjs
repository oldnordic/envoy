import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  buildMessagePath,
  normalizeMessagePayload,
  renderFileBusMessage,
  safeSubject,
} from "./envoy-listen.mjs";

describe("envoy-listen message normalization", () => {
  it("normalizes stored Envoy MessageEnvelope payloads", () => {
    const normalized = normalizeMessagePayload({
      message_id: "42",
      type: "direct",
      from: "claude2",
      to: "codex",
      context_id: "graph-memory",
      timestamp: "2026-05-07T09:20:00Z",
      sequence_id: 7,
      parts: [
        { text: "Appendix looks good." },
        { data: { thread: "graph-memory-architecture", subject: "brief review" } },
      ],
    });

    assert.deepEqual(normalized, {
      from: "claude2",
      to: "codex",
      subject: "brief review",
      body: "Appendix looks good.",
      thread: "graph-memory-architecture",
      timestamp: "2026-05-07T09:20:00Z",
      messageId: "42",
      sequenceId: 7,
    });
  });

  it("normalizes bridge message event payloads", () => {
    const normalized = normalizeMessagePayload({
      from: "hermes",
      to: "codex",
      subject: "Build envoy-listen daemon",
      body: "Connect to WS and write file bus messages.",
      thread: "envoy-transport",
      timestamp: "2026-05-07T09:11:07+02:00",
    });

    assert.equal(normalized.subject, "Build envoy-listen daemon");
    assert.equal(normalized.body, "Connect to WS and write file bus messages.");
    assert.equal(normalized.thread, "envoy-transport");
  });
});

describe("envoy-listen file bus output", () => {
  it("sanitizes subjects for filenames", () => {
    assert.equal(safeSubject("Build envoy-listen daemon!"), "build_envoy_listen_daemon");
    assert.equal(safeSubject("  ***  "), "message");
  });

  it("builds the expected from_to_receiver path", () => {
    const path = buildMessagePath("/tmp/messages", {
      from: "hermes",
      to: "codex",
      subject: "Build envoy-listen daemon!",
      timestamp: "2026-05-07T09:11:07+02:00",
    });

    assert.equal(path, "/tmp/messages/hermes_to_codex/20260507T091107_build_envoy_listen_daemon.md");
  });

  it("renders message bus markdown", () => {
    const markdown = renderFileBusMessage({
      from: "hermes",
      to: "codex",
      subject: "Build envoy-listen daemon",
      body: "Connect to WS and write files.",
      thread: "envoy-transport",
      timestamp: "2026-05-07T09:11:07+02:00",
      messageId: "42",
      sequenceId: 7,
    });

    assert.match(markdown, /^# Hermes -> Codex: Build envoy-listen daemon/m);
    assert.match(markdown, /^From: Hermes$/m);
    assert.match(markdown, /^Date: 2026-05-07T09:11:07\+02:00$/m);
    assert.match(markdown, /^Thread: envoy-transport$/m);
    assert.match(markdown, /Envoy-Message-ID: 42/);
    assert.match(markdown, /Envoy-Sequence-ID: 7/);
    assert.match(markdown, /Connect to WS and write files\./);
  });
});
