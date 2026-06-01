#!/usr/bin/env node
// E2E full-stack message delivery test for the Envoy plugin pipeline.
//
// What this tests:
//   1. Agent registration (POST /agents)
//   2. Send message from A to B (POST /messages)
//   3. B polls messages — message appears (GET /messages)
//   4. B polls AGAIN — message STILL appears (no auto-consumption)
//   5. B ACKs the message (POST /messages/{id}/ack)
//   6. B polls — message gone (ACK filters it)
//   7. B polls with include=acked — message reappears
//   8. Heartbeat resets circuit breaker on delivery failure
//
// This catches the seenIds bug: if step 4 fails (message disappears on
// second poll), the plugin's envoy_check was consuming messages.
//
// Usage: node tests/e2e-message-delivery.js [envoy-binary-path]

const http = require("http");
const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");
const os = require("os");

const ENVOY_BIN = process.argv[2] || "./target/release/envoy";
const HOST = "127.0.0.1";
let PORT = 19876; // Use a non-default port for testing
let serverProcess = null;
let tmpDir = null;
let passed = 0;
let failed = 0;

// ── HTTP helpers (mirrors envoy-channel.js exactly) ──

function httpGet(urlPath, params, headers = {}) {
  return new Promise((resolve, reject) => {
    let url = `http://${HOST}:${PORT}` + urlPath;
    if (params) {
      const qs = Object.entries(params)
        .filter(([, v]) => v !== undefined && v !== null)
        .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
        .join("&");
      if (qs) url += "?" + qs;
    }
    const req = http.get(url, { timeout: 3000, headers }, (res) => {
      let body = "";
      res.on("data", (chunk) => (body += chunk));
      res.on("end", () => {
        try { resolve({ status: res.statusCode, body: JSON.parse(body) }); }
        catch { resolve({ status: res.statusCode, body: body }); }
      });
    });
    req.on("error", (e) => reject(e));
    req.on("timeout", () => { req.destroy(); reject(new Error("timeout")); });
  });
}

function httpPost(urlPath, data, headers = {}) {
  return new Promise((resolve, reject) => {
    const url = new URL(`http://${HOST}:${PORT}` + urlPath);
    const body = JSON.stringify(data);
    const req = http.request(
      { hostname: url.hostname, port: url.port, path: url.pathname, method: "POST",
        headers: { "Content-Type": "application/json", "Content-Length": Buffer.byteLength(body), ...headers },
        timeout: 3000 },
      (res) => {
        let respBody = "";
        res.on("data", (chunk) => (respBody += chunk));
        res.on("end", () => {
          try { resolve({ status: res.statusCode, body: JSON.parse(respBody) }); }
          catch { resolve({ status: res.statusCode, body: respBody }); }
        });
      }
    );
    req.on("error", (e) => reject(e));
    req.on("timeout", () => { req.destroy(); reject(new Error("timeout")); });
    req.write(body);
    req.end();
  });
}

// ── Test helpers ──

function assert(condition, msg) {
  if (condition) {
    passed++;
    console.log(`  PASS: ${msg}`);
  } else {
    failed++;
    console.error(`  FAIL: ${msg}`);
  }
}

async function waitForServer(ms = 2000) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    try {
      await httpGet("/health");
      return;
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  throw new Error("Server did not start in time");
}

// ── Setup / Teardown ──

async function setup() {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "envoy-e2e-"));
  console.log(`\n=== E2E Message Delivery Test ===`);
  console.log(`Binary: ${ENVOY_BIN}`);
  console.log(`Temp DB: ${tmpDir}`);

  if (!fs.existsSync(ENVOY_BIN)) {
    console.error(`FATAL: envoy binary not found at ${ENVOY_BIN}`);
    process.exit(1);
  }

  // Start envoy server on test port
  serverProcess = spawn(ENVOY_BIN, [], {
    env: { ...process.env, ENVOY_PORT: String(PORT), ENVOY_DB: path.join(tmpDir, "test.db") },
    stdio: ["pipe", "pipe", "pipe"],
  });

  serverProcess.stdout.on("data", (d) => process.stdout.write(`[server] ${d}`));
  serverProcess.stderr.on("data", (d) => process.stderr.write(`[server] ${d}`));

  await waitForServer();
  console.log("Server started.\n");
}

async function teardown() {
  if (serverProcess) {
    serverProcess.kill("SIGTERM");
    await new Promise((r) => setTimeout(r, 500));
    serverProcess.kill("SIGKILL");
  }
  if (tmpDir) {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
  console.log(`\n=== Results: ${passed} passed, ${failed} failed ===`);
  process.exit(failed > 0 ? 1 : 0);
}

// ── Tests ──

async function testRegistration() {
  console.log("--- Test 1: Agent Registration ---");
  const resp = await httpPost("/agents", { name: "agent-a", kind: "worker" });
  assert(resp.status === 201, `agent-a registration returns 201 (got ${resp.status})`);
  assert(resp.body.agent_id === "id1", `agent-a gets id1 (got ${resp.body.agent_id})`);

  const resp2 = await httpPost("/agents", { name: "agent-b", kind: "worker" });
  assert(resp2.status === 201, `agent-b registration returns 201 (got ${resp2.status})`);
  assert(resp2.body.agent_id === "id2", `agent-b gets id2 (got ${resp2.body.agent_id})`);
}

async function testSendMessage() {
  console.log("\n--- Test 2: Send Message A → B ---");
  const resp = await httpPost("/messages", {
    type: "direct",
    from: "id1",
    to: "id2",
    parts: [{ text: "Test message from A to B" }],
    context_id: "e2e-test",
  }, { "x-agent-id": "id1" });
  assert(resp.status === 201, `message send returns 201 (got ${resp.status})`);
  assert(!!resp.body.message_id, `message has an ID`);
  return resp.body.message_id;
}

async function testPollShowsMessage() {
  console.log("\n--- Test 3: Poll Shows Unread Message ---");
  const resp = await httpGet("/messages", { to: "id2", since: 0, limit: 10 }, { "x-agent-id": "id2" });
  assert(resp.status === 200, `poll returns 200`);
  const msgs = resp.body.messages || [];
  assert(msgs.length === 1, `exactly 1 message for agent-b (got ${msgs.length})`);
  assert(msgs[0].from === "id1", `message from id1`);
  assert(msgs[0].context_id === "e2e-test", `subject matches`);
  return msgs[0].message_id;
}

async function testPollAgainStillShows(msgId) {
  console.log("\n--- Test 4: Poll AGAIN — Message Must Still Appear ---");
  // This is the seenIds test. Before the fix, a second poll would return 0
  // because envoy_check added to seenIds on the first call.
  const resp = await httpGet("/messages", { to: "id2", since: 0, limit: 10 }, { "x-agent-id": "id2" });
  const msgs = resp.body.messages || [];
  assert(msgs.length === 1, `message STILL appears on second poll (got ${msgs.length}) — seenIds bug check`);
  assert(msgs[0].message_id === msgId, `same message ID on second poll`);
}

async function testAckRemovesMessage(msgId) {
  console.log("\n--- Test 5: ACK Removes Message from Unacked Poll ---");
  const resp = await httpPost(`/messages/${msgId}/ack`, { agent_id: "id2" }, { "x-agent-id": "id2" });
  assert(resp.status === 200, `ACK returns 200`);
  assert(resp.body.acked_by && resp.body.acked_by.includes("id2"), `acked_by contains id2`);

  const pollResp = await httpGet("/messages", { to: "id2", since: 0, limit: 10 }, { "x-agent-id": "id2" });
  const msgs = pollResp.body.messages || [];
  assert(msgs.length === 0, `ACKed message gone from unacked poll (got ${msgs.length})`);
}

async function testAckedIncluded() {
  console.log("\n--- Test 6: Include=acked Shows ACKed Message ---");
  const resp = await httpGet("/messages", { to: "id2", since: 0, limit: 10, include: "acked" }, { "x-agent-id": "id2" });
  const msgs = resp.body.messages || [];
  assert(msgs.length === 1, `ACKed message visible with include=acked (got ${msgs.length})`);
}

async function testHeartbeatCircuitReset() {
  console.log("\n--- Test 7: Heartbeat Resets Circuit Breaker ---");
  try {
    // Record 5 failures to open the circuit
    for (let i = 0; i < 5; i++) {
      const failResp = await httpPost("/agents/id2/circuit/failure", {}, { "x-agent-id": "id2" });
      assert(failResp.status === 200, `failure ${i+1} returns 200 (got ${failResp.status})`);
    }
    const openResp = await httpGet("/agents/id2/circuit", undefined, { "x-agent-id": "id2" });
    const openState = openResp.body?.state || "unknown";
    assert(openState === "open", `circuit is open after 5 failures (got ${openState})`);

    // Send heartbeat — should reset circuit
    const hbResp = await httpPost("/heartbeat", {
      agent_id: "id2",
      status: { state: "working", working_on: "e2e-test" },
    }, { "x-agent-id": "id2" });
    assert(hbResp.status === 200, `heartbeat returns 200 (got ${hbResp.status})`);

    const closedResp = await httpGet("/agents/id2/circuit", undefined, { "x-agent-id": "id2" });
    const closedState = closedResp.body?.state || "unknown";
    const failCount = closedResp.body?.failure_count ?? -1;
    assert(closedState === "closed", `circuit closed after heartbeat (got ${closedState})`);
    assert(failCount === 0, `failure count reset to 0 (got ${failCount})`);
  } catch (e) {
    console.error(`  ERROR in test 7: ${e.message}`);
    console.error(`  Stack: ${e.stack}`);
    failed++;
  }
}

// ── Main ──

async function main() {
  try {
    await setup();
    await testRegistration();
    const msgId = await testSendMessage();
    await testPollShowsMessage();
    await testPollAgainStillShows(msgId);
    await testAckRemovesMessage(msgId);
    await testAckedIncluded();
    await testHeartbeatCircuitReset();
  } catch (e) {
    console.error(`\nFATAL: ${e.message}`);
    failed++;
  } finally {
    await teardown();
  }
}

main();
