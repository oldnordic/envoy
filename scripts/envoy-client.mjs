#!/usr/bin/env node
// envoy-client.mjs — Reference WS client for envoy coordination server
//
// Proves the full WS stack: connect, subscribe, heartbeat, reconnect
// with event catch-up replay. Preserves all provenance fields.
//
// Usage:
//   node scripts/envoy-client.mjs [--agent AGENT_ID] [--project PROJECT] [--url WS_URL]
//
// Defaults: agent=dogfood-client, project=envoy, url=ws://localhost:9876

const args = process.argv.slice(2);
function getArg(flag, defaultVal) {
  const idx = args.indexOf(flag);
  return idx >= 0 && idx + 1 < args.length ? args[idx + 1] : defaultVal;
}

const AGENT_NAME = getArg("--agent", "dogfood-client");
const PROJECT = getArg("--project", "envoy");
const WS_URL = getArg("--url", "ws://localhost:9876");
const HTTP_URL = WS_URL.replace("ws://", "http://").replace("wss://", "https://");
const HEARTBEAT_MS = 30_000;
const RECONNECT_MS = 5_000;

let agentId = null;
let ws = null;
let heartbeatTimer = null;
let reconnectAttempts = 0;
let lastEventSeq = null;

function log(tag, msg) {
  const ts = new Date().toISOString().slice(11, 19);
  const line = typeof msg === "object" ? JSON.stringify(msg) : msg;
  console.log(`[${ts}] [${tag}] ${line}`);
}

// Register agent + subscribe via HTTP before connecting WS
async function setup() {
  // Register
  const regRes = await fetch(`${HTTP_URL}/agents`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name: AGENT_NAME, kind: "worker" }),
  });
  if (!regRes.ok) {
    const text = await regRes.text();
    log("SETUP", `Register failed (${regRes.status}): ${text}`);
    return false;
  }
  const regData = await regRes.json();
  agentId = regData.agent_id;
  log("SETUP", `Registered agent: name=${AGENT_NAME} id=${agentId}`);

  // Subscribe to project
  const subRes = await fetch(`${HTTP_URL}/subscriptions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ agent_id: agentId, project: PROJECT }),
  });
  if (!subRes.ok) {
    const text = await subRes.text();
    log("SETUP", `Subscribe failed (${subRes.status}): ${text}`);
    return false;
  }
  log("SETUP", `Subscribed to project: ${PROJECT}`);
  return true;
}

function startHeartbeat() {
  stopHeartbeat();
  heartbeatTimer = setInterval(() => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      const hb = {
        type: "heartbeat",
        data: {
          state: "working",
          working_on: "dogfood WS client",
          waiting_for: null,
          can_start: true,
          checkpoint: "heartbeat",
        },
      };
      ws.send(JSON.stringify(hb));
      log("HEARTBEAT", "sent");
    }
  }, HEARTBEAT_MS);
}

function stopHeartbeat() {
  if (heartbeatTimer) {
    clearInterval(heartbeatTimer);
    heartbeatTimer = null;
  }
}

function handleEvent(msg) {
  const parsed = JSON.parse(msg);

  // Server-sent events are { event: "...", data: {...} }
  if (parsed.event) {
    const { event, data } = parsed;

    // Track sequence for catch-up replay
    if (data?.id) {
      lastEventSeq = data.id;
    }

    switch (event) {
      case "agent_connected":
        log("CONNECTED", `Agent ${data.agent_id} online`);
        break;
      case "message":
        log("MESSAGE", data);
        break;
      case "hook_event":
        log("HOOK", `${data.source} [${data.severity}]: ${data.message}`);
        break;
      case "gate_event":
        log("GATE", `${data.message} (data=${JSON.stringify(data.data)})`);
        break;
      case "ci_event":
        log("CI", `${data.source} [${data.severity}]: ${data.message}`);
        break;
      case "doc_event":
        log("DOC", `${data.source} [${data.severity}]: ${data.message}`);
        break;
      case "task_proposed":
        log("TASK", `Proposed: ${data.description} (${data.id})`);
        break;
      case "task_claimed":
        log("TASK", `Claimed: ${data.id} by ${data.claimed_by}`);
        break;
      case "task_state_changed":
        log("TASK", `State: ${data.id} → ${data.state} (${data.checkpoint || "no checkpoint"})`);
        break;
      case "event_catchup":
        log("CATCHUP", `Replayed event ${data.id} type=${data.event_type} sev=${data.severity}`);
        break;
      case "channel_lagged":
        log("LAGGED", `Skipped ${data.skipped} messages — re-subscribed`);
        break;
      case "nudge":
        log("NUDGE", `${data.severity}: ${data.reason}`);
        break;
      case "blocker_stale":
        log("BLOCKER", `${data.blocker_agent} may be stalled`);
        break;
      case "blocker_updated":
        log("BLOCKER", `Blocker ${data.blocker_agent} updated`);
        break;
      case "dependency_resolved":
        log("DEP", `Resolved: ${data.message}`);
        break;
      case "task_reclaimed":
        log("TASK", `Reclaimed: ${data.task_id} — ${data.message}`);
        break;
      default:
        log("EVENT", `${event}: ${JSON.stringify(data).slice(0, 120)}`);
    }
    return;
  }

  // Heartbeat ack
  if (parsed.type === "heartbeat_ack") {
    log("HEARTBEAT", `ack accepted=${parsed.data?.accepted} ts=${parsed.data?.timestamp}`);
    return;
  }

  // Pong
  if (parsed.type === "pong") {
    log("PING", "pong received");
    return;
  }

  // Unknown
  log("RECV", msg.slice(0, 200));
}

function connect() {
  const url = `${WS_URL}/ws/${agentId}`;
  log("WS", `Connecting to ${url} ...`);
  ws = new WebSocket(url);

  ws.onopen = () => {
    reconnectAttempts = 0;
    log("WS", "Connected");
    startHeartbeat();

    // Send initial ping
    ws.send(JSON.stringify({ type: "ping" }));
  };

  ws.onmessage = (ev) => {
    const msg = typeof ev.data === "string" ? ev.data : new TextDecoder().decode(ev.data);
    handleEvent(msg);
  };

  ws.onclose = (ev) => {
    stopHeartbeat();
    log("WS", `Closed (code=${ev.code} reason=${ev.reason || "none"})`);
    scheduleReconnect();
  };

  ws.onerror = (ev) => {
    log("WS", `Error: ${ev.message || "connection failed"}`);
  };
}

function scheduleReconnect() {
  reconnectAttempts++;
  const delay = Math.min(RECONNECT_MS * reconnectAttempts, 60_000);
  log("RECONNECT", `Attempting reconnect in ${delay}ms (attempt ${reconnectAttempts})`);
  setTimeout(() => {
    // Re-register before reconnecting (agent may have been cleaned up)
    setup().then((ok) => {
      if (ok) {
        connect();
      } else {
        log("RECONNECT", "Setup failed, retrying...");
        scheduleReconnect();
      }
    });
  }, delay);
}

// Main
async function main() {
  log("START", `envoy WS client — agent=${AGENT_NAME} project=${PROJECT} url=${WS_URL}`);

  const ok = await setup();
  if (!ok) {
    log("FATAL", "Setup failed, cannot continue");
    process.exit(1);
  }

  connect();
}

main();
