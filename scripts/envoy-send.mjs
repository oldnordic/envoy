#!/usr/bin/env node
// envoy-send.mjs — CLI wrapper for sending messages via envoy coordination server
//
// Usage:
//   envoy-send --from claude2 --to hermes --subject "status" --body "WS client done"
//   envoy-send --from claude2 --to claude1,codex --subject "fyi" --body "@file:/tmp/review.md"
//   envoy-send --from claude2 --to hermes --subject "report" --body "inline text" --json
//
// Zero dependencies — Node 25 native fetch + fs

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const args = process.argv.slice(2);

function getArg(flag) {
  const idx = args.indexOf(flag);
  return idx >= 0 && idx + 1 < args.length ? args[idx + 1] : null;
}

function hasFlag(flag) {
  return args.includes(flag);
}

function usage() {
  console.error(`Usage: envoy-send --from <agent> --to <agents> --subject <subj> --body <text|@file:path> [--json] [--url <url>]

Options:
  --from      Sender agent name (claude1, claude2, codex, hermes)
  --to        Recipient(s), comma-separated (claude1,claude2,codex,hermes)
  --subject   Message subject/thread
  --body      Message body, or @file:path to slurp a file
  --json      Output machine-readable JSON (message_id, timestamp, delivery)
  --url       Envoy server URL (default: http://localhost:9876)
  --help      Show this help

Examples:
  envoy-send --from claude2 --to hermes --subject "status" --body "task done"
  envoy-send --from claude2 --to claude1,codex --subject "fyi" --body "@file:/tmp/review.md"
  envoy-send --from hermes --to claude2 --subject "task" --body "build CLI" --json
`);
}

if (hasFlag("--help") || args.length === 0) {
  usage();
  process.exit(0);
}

const FROM = getArg("--from");
const TO = getArg("--to");
const SUBJECT = getArg("--subject");
const BODY = getArg("--body");
const JSON_OUTPUT = hasFlag("--json");
const BASE_URL = getArg("--url") || "http://localhost:9876";

if (!FROM || !TO || !SUBJECT || !BODY) {
  console.error("Error: --from, --to, --subject, and --body are required.");
  usage();
  process.exit(1);
}

const recipients = TO.split(",").map((s) => s.trim()).filter(Boolean);
if (recipients.length === 0) {
  console.error("Error: --to must contain at least one agent name.");
  process.exit(1);
}

// Resolve body — support @file:path syntax
let bodyText = BODY;
if (BODY.startsWith("@file:")) {
  const filePath = resolve(BODY.slice(6));
  try {
    bodyText = readFileSync(filePath, "utf-8");
  } catch (err) {
    console.error(`Error: cannot read file ${filePath}: ${err.message}`);
    process.exit(1);
  }
}

async function registerAgent(agentName) {
  const res = await fetch(`${BASE_URL}/agents`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name: agentName, kind: "worker" }),
  });
  if (!res.ok) {
    // Already registered is fine
    if (res.status === 409) return null;
    const text = await res.text();
    throw new Error(`Register ${agentName} failed (${res.status}): ${text}`);
  }
  const data = await res.json();
  return data.agent_id;
}

async function getAgentId(agentName) {
  // Try listing agents to find existing one
  const res = await fetch(`${BASE_URL}/agents`);
  if (!res.ok) throw new Error(`List agents failed (${res.status})`);
  const data = await res.json();
  const found = data.agents?.find(
    (a) => a.name === agentName || a.agent_id === agentName
  );
  if (found) return found.agent_id;

  // Register if not found
  const id = await registerAgent(agentName);
  return id;
}

async function sendMessage(fromId, toId, subject, body) {
  const res = await fetch(`${BASE_URL}/messages`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      type: "direct",
      from: fromId,
      to: toId,
      context_id: subject,
      parts: [{ text: body }],
    }),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Send to ${toId} failed (${res.status}): ${text}`);
  }
  return res.json();
}

async function main() {
  // Resolve sender
  const fromId = await getAgentId(FROM);

  // Send to each recipient
  const results = [];
  for (const recipient of recipients) {
    const toId = await getAgentId(recipient);
    const msg = await sendMessage(fromId, toId, SUBJECT, bodyText);
    results.push({ to: recipient, to_id: toId, ...msg });
  }

  if (JSON_OUTPUT) {
    console.log(JSON.stringify({ from: FROM, from_id: fromId, messages: results }, null, 2));
  } else {
    for (const r of results) {
      console.log(`Sent to ${r.to}: message_id=${r.message_id} seq=${r.sequence_id}`);
    }
  }
}

main().catch((err) => {
  console.error(`Error: ${err.message}`);
  process.exit(1);
});
