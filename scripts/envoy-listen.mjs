#!/usr/bin/env node
// envoy-listen.mjs - receive Envoy WS messages and mirror them to the file bus.
//
// Usage:
//   envoy-listen --agent codex --project agent-coordination
//   envoy-listen --agent codex --url ws://localhost:9876 --root /home/feanor/Projects/messages

import { mkdirSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_WS_URL = "ws://localhost:9876";
const DEFAULT_PROJECT = "agent-coordination";
const DEFAULT_MESSAGES_ROOT = "/home/feanor/Projects/messages";
const HEARTBEAT_MS = 30_000;
const RECONNECT_MS = 5_000;

function getArg(args, flag, defaultValue = null) {
  const idx = args.indexOf(flag);
  return idx >= 0 && idx + 1 < args.length ? args[idx + 1] : defaultValue;
}

function hasFlag(args, flag) {
  return args.includes(flag);
}

function usage() {
  console.error(`Usage: envoy-listen --agent <agent> [--project <project>] [--url <ws-url>] [--messages-root <path>]

Options:
  --agent          Agent name or id to receive as (required)
  --project        Project subscription name (default: ${DEFAULT_PROJECT})
  --url            Envoy WebSocket URL (default: ${DEFAULT_WS_URL})
  --messages-root  File bus root (default: ${DEFAULT_MESSAGES_ROOT})
  --root           Alias for --messages-root
  --help           Show this help
`);
}

function titleAgent(agent) {
  if (!agent) return "Unknown";
  return agent
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => part.slice(0, 1).toUpperCase() + part.slice(1))
    .join("");
}

export function safeSubject(subject) {
  const cleaned = String(subject || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .replace(/_{2,}/g, "_")
    .slice(0, 80);
  return cleaned || "message";
}

function timestampForFilename(timestamp) {
  const raw = String(timestamp || new Date().toISOString());
  const compact = raw
    .replace(/\.\d+/, "")
    .replace(/[-:]/g, "")
    .replace(/\+.*/, "")
    .replace(/Z$/, "");
  const match = compact.match(/^(\d{8})T?(\d{6})/);
  if (match) return `${match[1]}T${match[2]}`;
  return new Date().toISOString().replace(/\.\d+Z$/, "").replace(/[-:]/g, "");
}

export function buildMessagePath(messagesRoot, message) {
  const from = safeSubject(message.from);
  const to = safeSubject(message.to);
  const dir = `${from}_to_${to}`;
  const filename = `${timestampForFilename(message.timestamp)}_${safeSubject(message.subject)}.md`;
  return join(messagesRoot, dir, filename);
}

function partText(part) {
  if (!part || typeof part !== "object") return "";
  if (typeof part.text === "string") return part.text;
  if (part.data !== undefined) return "";
  if (typeof part.url === "string") return part.url;
  return "";
}

function partData(part) {
  if (!part || typeof part !== "object") return null;
  return part.data && typeof part.data === "object" ? part.data : null;
}

export function normalizeMessagePayload(payload, agentNameById = {}) {
  if (!payload || typeof payload !== "object") {
    throw new Error("message payload must be an object");
  }

  const from = agentNameById[payload.from] || payload.from;
  const to = agentNameById[payload.to] || payload.to;
  const parts = Array.isArray(payload.parts) ? payload.parts : [];
  const dataParts = parts.map(partData).filter(Boolean);
  const mergedData = Object.assign({}, ...dataParts);
  const bodyFromParts = parts.map(partText).filter(Boolean).join("\n\n").trim();

  const subject =
    payload.subject ||
    mergedData.subject ||
    payload.context_id ||
    payload.task_id ||
    payload.message ||
    "message";
  const thread = payload.thread || mergedData.thread || payload.context_id || subject;
  const body = payload.body || bodyFromParts || mergedData.body || "";

  if (!from || !to) {
    throw new Error("message payload requires from and to");
  }

  return {
    from,
    to,
    subject,
    body,
    thread,
    timestamp: payload.timestamp || new Date().toISOString(),
    messageId: payload.message_id || payload.id || null,
    sequenceId: payload.sequence_id ?? null,
  };
}

export function renderFileBusMessage(message) {
  const lines = [
    `# ${titleAgent(message.from)} -> ${titleAgent(message.to)}: ${message.subject}`,
    "",
    `From: ${titleAgent(message.from)}`,
    `Date: ${message.timestamp}`,
    `Thread: ${message.thread || message.subject}`,
  ];

  if (message.messageId) lines.push(`Envoy-Message-ID: ${message.messageId}`);
  if (message.sequenceId !== null && message.sequenceId !== undefined) {
    lines.push(`Envoy-Sequence-ID: ${message.sequenceId}`);
  }

  lines.push("", "---", "", message.body || "", "", "[End of message]", "");
  return lines.join("\n");
}

function log(tag, message) {
  const ts = new Date().toISOString();
  const text = typeof message === "object" ? JSON.stringify(message) : message;
  console.error(`[${ts}] [${tag}] ${text}`);
}

function httpUrlFromWs(wsUrl) {
  return wsUrl.replace(/^ws:\/\//, "http://").replace(/^wss:\/\//, "https://");
}

async function fetchJson(url, options = {}) {
  const res = await fetch(url, options);
  if (!res.ok) {
    const body = await res.text();
    throw new Error(`${options.method || "GET"} ${url} failed (${res.status}): ${body}`);
  }
  return res.json();
}

async function listAgents(httpUrl) {
  const data = await fetchJson(`${httpUrl}/agents`);
  return Array.isArray(data.agents) ? data.agents : [];
}

async function registerAgent(httpUrl, agentName) {
  const data = await fetchJson(`${httpUrl}/agents`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name: agentName, kind: "worker" }),
  });
  return data.agent_id;
}

async function resolveAgent(httpUrl, agentName) {
  const agents = await listAgents(httpUrl);
  const found = agents.find((agent) => agent.name === agentName || agent.agent_id === agentName);
  if (found) return found.agent_id;
  return registerAgent(httpUrl, agentName);
}

async function agentNameMap(httpUrl) {
  const agents = await listAgents(httpUrl);
  return Object.fromEntries(agents.map((agent) => [agent.agent_id, agent.name || agent.agent_id]));
}

async function subscribe(httpUrl, agentId, project) {
  await fetchJson(`${httpUrl}/subscriptions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ agent_id: agentId, project }),
  });
}

function writeMessageFile(messagesRoot, message) {
  const target = buildMessagePath(messagesRoot, message);
  if (existsSync(target)) {
    log("SKIP", `already exists: ${target}`);
    return target;
  }
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, renderFileBusMessage(message), { encoding: "utf8", flag: "wx" });
  log("WRITE", target);
  return target;
}

function heartbeatPayload(checkpoint) {
  return {
    type: "heartbeat",
    data: {
      state: "working",
      working_on: "envoy-listen daemon",
      waiting_for: null,
      can_start: true,
      checkpoint,
    },
  };
}

class EnvoyListener {
  constructor({ agentName, project, wsUrl, messagesRoot }) {
    this.agentName = agentName;
    this.project = project;
    this.wsUrl = wsUrl;
    this.httpUrl = httpUrlFromWs(wsUrl);
    this.messagesRoot = messagesRoot;
    this.agentId = null;
    this.ws = null;
    this.heartbeatTimer = null;
    this.reconnectAttempts = 0;
    this.agentNames = {};
  }

  async setup() {
    this.agentId = await resolveAgent(this.httpUrl, this.agentName);
    await subscribe(this.httpUrl, this.agentId, this.project);
    this.agentNames = await agentNameMap(this.httpUrl);
    log("SETUP", `agent=${this.agentName} id=${this.agentId} project=${this.project}`);
  }

  startHeartbeat() {
    this.stopHeartbeat();
    this.heartbeatTimer = setInterval(() => {
      this.send(heartbeatPayload("envoy-listen heartbeat"));
    }, HEARTBEAT_MS);
  }

  stopHeartbeat() {
    if (this.heartbeatTimer) {
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  send(payload) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(payload));
    }
  }

  async handleFrame(raw) {
    const parsed = JSON.parse(raw);
    if (parsed.event === "message") {
      this.agentNames = await agentNameMap(this.httpUrl).catch(() => this.agentNames);
      const message = normalizeMessagePayload(parsed.data, this.agentNames);
      writeMessageFile(this.messagesRoot, message);
      this.send(heartbeatPayload(`wrote message ${message.messageId || message.subject}`));
      return;
    }

    if (parsed.event === "agent_connected") {
      log("CONNECTED", parsed.data);
      return;
    }

    if (parsed.type === "heartbeat_ack" || parsed.type === "pong") {
      log("ACK", parsed);
      return;
    }

    log("IGNORE", parsed.event || parsed.type || "unknown");
  }

  connect() {
    const url = `${this.wsUrl}/ws/${this.agentId}`;
    log("WS", `connecting ${url}`);
    this.ws = new WebSocket(url);

    this.ws.onopen = () => {
      this.reconnectAttempts = 0;
      this.startHeartbeat();
      this.send({ type: "ping" });
      log("WS", "connected");
    };

    this.ws.onmessage = (event) => {
      const raw = typeof event.data === "string" ? event.data : new TextDecoder().decode(event.data);
      this.handleFrame(raw).catch((err) => log("ERROR", err.message));
    };

    this.ws.onclose = (event) => {
      this.stopHeartbeat();
      log("WS", `closed code=${event.code} reason=${event.reason || "none"}`);
      this.scheduleReconnect();
    };

    this.ws.onerror = (event) => {
      log("WS", event.message || "connection error");
    };
  }

  scheduleReconnect() {
    this.reconnectAttempts += 1;
    const delay = Math.min(RECONNECT_MS * this.reconnectAttempts, 60_000);
    log("RECONNECT", `retry in ${delay}ms attempt=${this.reconnectAttempts}`);
    setTimeout(async () => {
      try {
        await this.setup();
        this.connect();
      } catch (err) {
        log("RECONNECT", err.message);
        this.scheduleReconnect();
      }
    }, delay);
  }

  async run() {
    await this.setup();
    this.connect();
  }
}

async function main(argv = process.argv.slice(2)) {
  if (hasFlag(argv, "--help") || argv.length === 0) {
    usage();
    process.exit(0);
  }

  const agentName = getArg(argv, "--agent");
  if (!agentName) {
    console.error("Error: --agent is required.");
    usage();
    process.exit(1);
  }

  const listener = new EnvoyListener({
    agentName,
    project: getArg(argv, "--project", DEFAULT_PROJECT),
    wsUrl: getArg(argv, "--url", DEFAULT_WS_URL).replace(/\/$/, ""),
    messagesRoot: getArg(argv, "--messages-root", getArg(argv, "--root", DEFAULT_MESSAGES_ROOT)),
  });
  await listener.run();
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  main().catch((err) => {
    log("FATAL", err.message);
    process.exit(1);
  });
}
