//! envoy-hook — shell-agnostic Claude Code hook binary
//!
//! Reads CLAUDE_SESSION_ID + CLAUDE_PROJECT_DIR from the environment,
//! reads tool JSON from stdin where applicable, and POSTs to the local
//! Envoy service which persists events to Atheneum.
//!
//! Auth: registers a persistent "claude-code-hooks" agent on first use and
//! caches the agent-id in ~/.local/share/envoy/hook-agent-id. All subsequent
//! calls send X-Agent-Id with that value.
//!
//! Never exits non-zero — hooks must not break the coding workflow.
//!
//! Usage in ~/.claude/settings.json:
//!   SessionStart  → envoy-hook session-start
//!   PostToolUse   → envoy-hook tool-call
//!   Stop          → envoy-hook session-end
//!   SubagentStop  → envoy-hook session-end

use std::io::Read;
use std::path::PathBuf;

use ahash::AHasher;
use serde_json::{json, Value};
use std::hash::{Hash, Hasher};

const DEFAULT_ENVOY: &str = "http://127.0.0.1:9876";
const AGENT_NAME: &str = "claude-code-hooks";
const AGENT_KIND: &str = "hook";

fn envoy_url() -> String {
    std::env::var("ENVOY_URL").unwrap_or_else(|_| DEFAULT_ENVOY.to_string())
}

fn session_id() -> Option<String> {
    std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
}

fn project_dir() -> String {
    std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

fn project_name(dir: &str) -> String {
    // Prefer the git toplevel basename so worktrees, subdirectories, and
    // launches from a subdir all map to the repository name — not the cwd's
    // immediate parent (which produced `tmp` and subdir tags historically).
    // Falls back to the dir basename when not inside a git worktree.
    let toplevel = std::process::Command::new("git")
        .args(["-C", dir, "rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    let base = toplevel.as_deref().unwrap_or(dir);
    std::path::Path::new(base)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string())
}

fn git_info(dir: &str) -> (Option<String>, Option<String>) {
    let branch = std::process::Command::new("git")
        .args(["-C", dir, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    let head = std::process::Command::new("git")
        .args(["-C", dir, "rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    (branch, head)
}

fn ahash_hex(s: &str) -> String {
    let mut h = AHasher::default();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        format!("{}…", chars[..max].iter().collect::<String>())
    }
}

fn tool_category(tool: &str) -> &'static str {
    match tool {
        "Bash" => "shell",
        "Read" => "file_read",
        "Write" | "Edit" => "file_write",
        "Agent" => "agent",
        "WebFetch" | "WebSearch" => "network",
        "Glob" | "Grep" | "LS" => "file_read",
        _ => "other",
    }
}

fn summarize_input(tool: &str, input: &Value) -> String {
    let ti = input.get("tool_input").unwrap_or(input);
    match tool {
        "Bash" => ti
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, 140))
            .unwrap_or_else(|| "bash".to_string()),
        "Read" => ti
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(|s| format!("read {s}"))
            .unwrap_or_else(|| "read".to_string()),
        "Write" | "Edit" => ti
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(|s| format!("write {s}"))
            .unwrap_or_else(|| "write".to_string()),
        "Glob" => ti
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|s| format!("glob {s}"))
            .unwrap_or_else(|| "glob".to_string()),
        "Grep" => ti
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|s| format!("grep {s}"))
            .unwrap_or_else(|| "grep".to_string()),
        _ => truncate(&ti.to_string(), 140),
    }
}

fn summarize_response(_tool: &str, resp: &Value) -> String {
    // Claude Code PostToolUse format: {stdout, stderr, interrupted, ...}
    if let Some(stdout) = resp.get("stdout").and_then(|v| v.as_str()) {
        if !stdout.is_empty() {
            let stderr = resp.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
            if stderr.is_empty() {
                return truncate(stdout, 200);
            }
            return truncate(&format!("{stdout}\n[stderr: {stderr}]"), 200);
        }
        if let Some(stderr) = resp.get("stderr").and_then(|v| v.as_str()) {
            if !stderr.is_empty() {
                return format!("stderr: {}", truncate(stderr, 190));
            }
        }
        return "(no output)".to_string();
    }
    // Generic fallbacks
    if let Some(s) = resp.as_str() {
        return truncate(s, 200);
    }
    if let Some(output) = resp.get("output").and_then(|v| v.as_str()) {
        return truncate(output, 200);
    }
    if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
        return format!("error: {}", truncate(err, 180));
    }
    truncate(&resp.to_string(), 200)
}

// ── Extra-emission helpers ────────────────────────────────────────────────────

/// Extract the file_path from a Write/Edit tool payload.
fn extract_file_path(tool: &str, payload: &Value) -> Option<String> {
    match tool {
        "Write" | "Edit" => {
            let ti = payload.get("tool_input").unwrap_or(payload);
            ti.get("file_path")
                .and_then(|v| v.as_str())
                .map(String::from)
        }
        _ => None,
    }
}

fn extract_accessed_paths(tool: &str, payload: &Value) -> Vec<String> {
    let ti = payload.get("tool_input").unwrap_or(payload);
    let mut paths = Vec::new();

    match tool {
        "Read" | "LS" | "Glob" | "Grep" => {
            if let Some(path) = ti.get("file_path").and_then(|v| v.as_str()) {
                paths.push(path.to_string());
            }
            if let Some(path) = ti.get("path").and_then(|v| v.as_str()) {
                paths.push(path.to_string());
            }
            if let Some(items) = ti.get("paths").and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(path) = item.as_str() {
                        paths.push(path.to_string());
                    }
                }
            }
        }
        _ => {}
    }

    paths.sort();
    paths.dedup();
    paths
}

fn relation_endpoint(kind: &str, name: String, file_path: Option<String>, data: Value) -> Value {
    json!({
        "kind": kind,
        "name": name,
        "file_path": file_path,
        "data": data,
    })
}

fn session_endpoint(session_id: &str) -> Value {
    relation_endpoint(
        "Session",
        format!("claude-code:{session_id}"),
        None,
        json!({ "session_id": session_id }),
    )
}

fn file_endpoint(path: &str) -> Value {
    relation_endpoint(
        "File",
        path.to_string(),
        Some(path.to_string()),
        json!({ "file_path": path }),
    )
}

fn project_endpoint(project: &str) -> Value {
    relation_endpoint(
        "Project",
        project.to_string(),
        None,
        json!({ "project_id": project }),
    )
}

fn failure_endpoint(tool_name: &str) -> Value {
    relation_endpoint(
        "Failure",
        format!("tool:{tool_name}:error"),
        None,
        json!({ "tool_name": tool_name, "failure_type": "tool_error" }),
    )
}

fn tool_relation_events(
    session_id: &str,
    project: &str,
    tool_name: &str,
    payload: &Value,
    exit_status: &str,
) -> Vec<Value> {
    let mut events = Vec::new();
    let session = session_endpoint(session_id);

    for path in extract_accessed_paths(tool_name, payload) {
        let file = file_endpoint(&path);
        events.push(json!({
            "session_id": session_id,
            "event_type": "tool_relation",
            "entity_id": format!("{session_id}:{tool_name}:{path}:accessed"),
            "payload": {
                "tool_name": tool_name,
                "file_path": path,
                "relations": [
                    {
                        "from": session.clone(),
                        "to": file.clone(),
                        "edge_type": "accessed",
                        "data": { "tool_name": tool_name, "source": "envoy-hook" }
                    },
                    {
                        "from": file,
                        "to": project_endpoint(project),
                        "edge_type": "belongs_to_project",
                        "data": { "source": "envoy-hook" }
                    }
                ]
            }
        }));
    }

    if exit_status == "success" {
        if let Some(path) = extract_file_path(tool_name, payload) {
            let file = file_endpoint(&path);
            events.push(json!({
                "session_id": session_id,
                "event_type": "tool_relation",
                "entity_id": format!("{session_id}:{tool_name}:{path}:modified"),
                "payload": {
                    "tool_name": tool_name,
                    "file_path": path,
                    "relations": [
                        {
                            "from": session.clone(),
                            "to": file.clone(),
                            "edge_type": "modified",
                            "data": { "tool_name": tool_name, "source": "envoy-hook" }
                        },
                        {
                            "from": file,
                            "to": project_endpoint(project),
                            "edge_type": "belongs_to_project",
                            "data": { "source": "envoy-hook" }
                        }
                    ]
                }
            }));
        }
    }

    if exit_status == "error" {
        events.push(json!({
            "session_id": session_id,
            "event_type": "tool_failure",
            "entity_id": format!("{session_id}:{tool_name}:error"),
            "payload": {
                "tool_name": tool_name,
                "relations": [
                    {
                        "from": failure_endpoint(tool_name),
                        "to": session.clone(),
                        "edge_type": "observed_in",
                        "data": { "source": "envoy-hook" }
                    },
                    {
                        "from": failure_endpoint(tool_name),
                        "to": project_endpoint(project),
                        "edge_type": "belongs_to_project",
                        "data": { "source": "envoy-hook" }
                    }
                ]
            }
        }));
    }

    events
}

fn infer_test_suite(command: &str) -> Option<&'static str> {
    let trimmed = command.trim();
    if trimmed.starts_with("cargo test") {
        Some("cargo")
    } else if trimmed.starts_with("pytest") || trimmed.starts_with("python -m pytest") {
        Some("pytest")
    } else if trimmed.starts_with("go test") {
        Some("go")
    } else if trimmed.starts_with("npm test")
        || trimmed.starts_with("pnpm test")
        || trimmed.starts_with("yarn test")
        || trimmed.starts_with("bun test")
    {
        Some("js-test")
    } else {
        None
    }
}

fn maybe_test_run_body(
    session_id: &str,
    tool_name: &str,
    payload: &Value,
    exit_status: &str,
    latency_ms: i64,
    output_summary: Option<&str>,
) -> Option<Value> {
    if tool_name != "Bash" {
        return None;
    }

    let command = payload
        .get("tool_input")
        .unwrap_or(payload)
        .get("command")
        .and_then(|v| v.as_str())?;
    let suite = infer_test_suite(command)?;

    Some(json!({
        "session_id": session_id,
        "test_name": truncate(command, 200),
        "test_suite": suite,
        "test_command": command,
        "result": if exit_status == "success" { "passed" } else { "failed" },
        "duration_ms": latency_ms,
        "logs_summary": output_summary,
        "commit_sha": null,
    }))
}

/// Find the magellan DB for the current project directory.
fn magellan_db_path(dir: &str) -> Option<String> {
    let name = project_name(dir);
    let candidates = [
        format!("{dir}/.magellan/{name}.db"),
        format!("{dir}/.magellan/{}.db", name.replace('-', "_")),
    ];
    candidates
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
}

/// Parse `git diff --numstat HEAD` into (file, added, deleted) triples.
fn git_diff_numstat(dir: &str) -> Vec<(String, u32, u32)> {
    let out = std::process::Command::new("git")
        .args(["-C", dir, "diff", "--numstat", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    out.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() == 3 {
                let added = parts[0].parse::<u32>().unwrap_or(0);
                let deleted = parts[1].parse::<u32>().unwrap_or(0);
                Some((parts[2].to_string(), added, deleted))
            } else {
                None
            }
        })
        .take(50)
        .collect()
}

// ── Agent ID caching ─────────────────────────────────────────────────────────

fn dirs_next() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local").join("share")
        })
}

fn hook_agent_id_path() -> PathBuf {
    dirs_next().join("envoy").join("hook-agent-id")
}

fn session_agent_id_path(session_id: &str) -> PathBuf {
    dirs_next()
        .join("envoy")
        .join("sessions")
        .join(format!("{}.agent-id", session_id))
}

fn verify_agent_active(id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    ureq::get(&format!("{}/agents/{}", envoy_url(), id))
        .call()
        .map(|r| r.status() == 200)
        .unwrap_or(false)
}

fn register_agent(name: &str, kind: &str) -> Result<String, Box<dyn std::error::Error>> {
    let resp = ureq::post(&format!("{}/agents", envoy_url()))
        .set("Content-Type", "application/json")
        .send_string(&json!({"name": name, "kind": kind}).to_string())?;
    let body: Value = serde_json::from_str(&resp.into_string()?)?;
    body.get("agent_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "no agent_id in registration response".into())
}

fn write_cache(path: &PathBuf, id: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, id).ok();
}

/// Hook agent — persistent single identity for hook API calls.
/// Used by tool-call / session-end commands that don't need per-session identity.
fn agent_id() -> Result<String, Box<dyn std::error::Error>> {
    let path = hook_agent_id_path();
    if path.exists() {
        let id = std::fs::read_to_string(&path)?.trim().to_string();
        if verify_agent_active(&id) {
            return Ok(id);
        }
    }
    let id = register_agent(AGENT_NAME, AGENT_KIND)?;
    write_cache(&path, &id);
    Ok(id)
}

/// Session agent — per-session identity exposed to the LLM via GROUNDED_AGENT_ID.
///
/// Check order:
///   1. GROUNDED_AGENT_ID env var (already set this session) — verify still active
///   2. Per-session cache file ~/.local/share/envoy/sessions/{sid}.agent-id
///   3. Register fresh, named claude-main or claude-sub{N}
///
/// Writes the id back to CLAUDE_ENV_FILE so the LLM can read $GROUNDED_AGENT_ID.
fn session_agent_id(
    session_id: &str,
    is_subagent: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    // 1. Check env var — non-empty and still active means we already registered
    let env_id = std::env::var("GROUNDED_AGENT_ID").unwrap_or_default();
    if !env_id.is_empty() && verify_agent_active(&env_id) {
        return Ok(env_id);
    }

    // 2. Check per-session cache file
    let cache_path = session_agent_id_path(session_id);
    if cache_path.exists() {
        let id = std::fs::read_to_string(&cache_path)?.trim().to_string();
        if !id.is_empty() && verify_agent_active(&id) {
            export_agent_id(&id);
            return Ok(id);
        }
    }

    // 3. Register fresh
    // Count existing claude-sub agents to derive the next number
    let name = if is_subagent {
        let count = count_active_sub_agents().unwrap_or(0);
        format!("claudesub{}", count + 1)
    } else {
        "claude-main".to_string()
    };

    let id = register_agent(&name, "claude")?;
    write_cache(&cache_path, &id);
    export_agent_id(&id);
    Ok(id)
}

/// Count active claude-sub* agents to determine next subagent number.
fn count_active_sub_agents() -> Result<usize, Box<dyn std::error::Error>> {
    let resp = ureq::get(&format!("{}/agents", envoy_url())).call()?;
    let body: Value = serde_json::from_str(&resp.into_string()?)?;
    let count = body
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|a| {
                    a.get("name")
                        .and_then(|v| v.as_str())
                        .map(|n| n.starts_with("claudesub"))
                        .unwrap_or(false)
                        && a.get("lifecycle")
                            .and_then(|v| v.as_str())
                            .map(|s| s == "active")
                            .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);
    Ok(count)
}

/// Write GROUNDED_AGENT_ID to CLAUDE_ENV_FILE so LLM sees it in subsequent tool calls.
fn export_agent_id(id: &str) {
    if let Ok(env_file) = std::env::var("CLAUDE_ENV_FILE") {
        if !env_file.is_empty() {
            let line = format!("export GROUNDED_AGENT_ID={}\n", id);
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&env_file) {
                f.write_all(line.as_bytes()).ok();
            }
        }
    }
}

// ── HTTP helpers ─────────────────────────────────────────────────────────────

fn post_auth(url: &str, agent: &str, body: Value) -> Result<(), Box<dyn std::error::Error>> {
    ureq::post(url)
        .set("Content-Type", "application/json")
        .set("X-Agent-Id", agent)
        .send_string(&body.to_string())?;
    Ok(())
}

fn patch_auth(url: &str, agent: &str, body: Value) -> Result<(), Box<dyn std::error::Error>> {
    ureq::request("PATCH", url)
        .set("Content-Type", "application/json")
        .set("X-Agent-Id", agent)
        .send_string(&body.to_string())?;
    Ok(())
}

#[derive(Debug)]
struct SessionData {
    last_tool_call: Option<String>,
    last_summary: Option<String>,
}

/// GET helper for authenticated requests to envoy
fn get_auth(url: &str, agent: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let response = ureq::get(url).set("X-Agent-Id", agent).call()?;

    let body = response.into_string()?;
    Ok(serde_json::from_str(&body)?)
}

/// Query session data from envoy for auto-discovery
fn get_session_data(
    session_id: &str,
    agent: &str,
) -> Result<SessionData, Box<dyn std::error::Error>> {
    let url = &format!("{}/atheneum/sessions/{}", envoy_url(), session_id);
    let response = get_auth(url, agent)?;

    // Extract last tool call and summary from session events
    let session = response.get("session").and_then(|s| s.as_object());
    let events = session
        .and_then(|s| s.get("events"))
        .and_then(|e| e.as_array());

    let (last_tool, last_summary) = if let Some(evs) = events {
        // Find last tool_call event
        evs.iter()
            .filter(|e| e.get("event_type").and_then(|t| t.as_str()) == Some("tool_call"))
            .last()
            .map_or((None, None), |event| {
                let payload = event.get("payload").and_then(|p| p.as_object());
                let tool = payload
                    .and_then(|p| p.get("tool_name"))
                    .and_then(|t| t.as_str())
                    .map(String::from);
                let summary = payload
                    .and_then(|p| p.get("output_summary"))
                    .and_then(|s| s.as_str())
                    .map(String::from);
                (tool, summary)
            })
    } else {
        (None, None)
    };

    Ok(SessionData {
        last_tool_call: last_tool,
        last_summary: last_summary,
    })
}

/// Check if discovery already exists for session+target (dedup)
fn discovery_exists_for_session(
    session_id: &str,
    target: &str,
    agent: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let url = &format!(
        "{}/atheneum/discoveries/recent?session_id={}&limit=50",
        envoy_url(),
        session_id
    );
    let response = get_auth(url, agent)?;

    if let Some(discoveries) = response.get("discoveries").and_then(|d| d.as_array()) {
        Ok(discoveries.iter().any(|d| {
            d.get("target")
                .and_then(|t| t.as_str())
                .map(|t| t == target)
                .unwrap_or(false)
        }))
    } else {
        Ok(false)
    }
}

// ── Commands ─────────────────────────────────────────────────────────────────

fn cmd_session_start() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).ok();
    let stdin: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

    let sid = stdin
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(session_id)
        .filter(|s| !s.is_empty());
    let sid = match sid {
        Some(s) => s,
        None => return Ok(()),
    };
    let parent_sid = std::env::var("CLAUDE_PARENT_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let is_subagent = parent_sid.is_some();

    // Register (or reuse) per-session agent; writes GROUNDED_AGENT_ID to CLAUDE_ENV_FILE
    let session_aid =
        session_agent_id(&sid, is_subagent).unwrap_or_else(|_| agent_id().unwrap_or_default());
    if session_aid.is_empty() {
        return Ok(());
    }

    // Use cwd from stdin if available (more reliable than env in some contexts)
    let dir = stdin
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(project_dir);
    let project = project_name(&dir);
    let (branch, head) = git_info(&dir);
    let model = std::env::var("CLAUDE_MODEL").ok();
    let trigger = if is_subagent { "subagent" } else { "cli" };

    let body = json!({
        "session_id":        sid,
        "agent":             AGENT_NAME,
        "project":           project,
        "tool":              "claude-code",
        "trigger":           trigger,
        "model":             model,
        "git_branch":        branch,
        "git_head":          head,
        "parent_session_id": parent_sid,
    });

    post_auth(
        &format!("{}/atheneum/sessions", envoy_url()),
        &session_aid,
        body,
    )
}

fn cmd_tool_call() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).ok();
    let payload: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

    // session_id: prefer stdin JSON (always present in PostToolUse), fallback to env
    let sid = payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(session_id)
        .filter(|s| !s.is_empty());
    let sid = match sid {
        Some(s) => s,
        None => return Ok(()),
    };
    let aid = agent_id()?;
    let dir = payload
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(project_dir);
    let project = project_name(&dir);

    let tool_name = payload
        .get("tool_name")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "unknown".to_string());

    let input_summary = summarize_input(&tool_name, &payload);
    let input_hash = ahash_hex(&payload.to_string());
    let latency_ms = payload
        .get("duration_ms")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let tool_response = payload.get("tool_response").or(payload.get("tool_result"));
    let (output_summary, output_hash, exit_status) = match tool_response {
        Some(r) => {
            let is_error = r
                .get("type")
                .and_then(|v| v.as_str())
                .map(|t| t == "error" || t == "tool_error")
                .unwrap_or(false)
                || r.get("interrupted")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
            (
                Some(summarize_response(&tool_name, r)),
                Some(ahash_hex(&r.to_string())),
                if is_error { "error" } else { "success" },
            )
        }
        None => (None, None, "unknown"),
    };

    // Capture extras before values are moved into body.
    let is_file_tool = matches!(tool_name.as_str(), "Write" | "Edit");
    let is_error = exit_status == "error";
    let file_path = if is_file_tool {
        extract_file_path(&tool_name, &payload)
    } else {
        None
    };
    let write_type = if tool_name == "Write" {
        "create"
    } else {
        "edit"
    };
    let disc_body = if is_error {
        Some(json!({
            "agent": AGENT_NAME,
            "discovery_type": "ToolFailure",
            "target": format!("tool:{tool_name}"),
            "metadata": {
                "session_id": &sid,
                "input_summary": &input_summary,
                "output_summary": output_summary.as_deref().unwrap_or(""),
            }
        }))
    } else {
        None
    };
    let relation_events = tool_relation_events(&sid, &project, &tool_name, &payload, exit_status);
    let test_run_body = maybe_test_run_body(
        &sid,
        &tool_name,
        &payload,
        exit_status,
        latency_ms,
        output_summary.as_deref(),
    );

    let body = json!({
        "session_id":      sid,
        "tool_name":       tool_name,
        "tool_version":    null,
        "input_hash":      input_hash,
        "input_summary":   input_summary,
        "output_hash":     output_hash,
        "output_summary":  output_summary,
        "exit_status":     exit_status,
        "latency_ms":      latency_ms,
        "input_tokens_est": null,
        "tool_category":   tool_category(&tool_name),
    });

    let result = post_auth(&format!("{}/atheneum/tool-calls", envoy_url()), &aid, body);

    // Emit file-write relation for Write/Edit on success.
    if let Some(fp) = file_path {
        post_auth(
            &format!("{}/atheneum/file-writes", envoy_url()),
            &aid,
            json!({
                "session_id": &sid,
                "file_path":  fp,
                "write_type": write_type,
            }),
        )
        .ok();
    }

    if let Some(test_body) = test_run_body {
        post_auth(
            &format!("{}/atheneum/test-runs", envoy_url()),
            &aid,
            test_body,
        )
        .ok();
    }

    for event in relation_events {
        post_auth(&format!("{}/atheneum/events", envoy_url()), &aid, event).ok();
    }

    // Emit failure discovery on tool error.
    if let Some(disc) = disc_body {
        post_auth(&format!("{}/atheneum/discoveries", envoy_url()), &aid, disc).ok();
    }

    result
}

fn cmd_session_end() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).ok();
    let payload: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

    let sid = payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(session_id)
        .filter(|s| !s.is_empty());
    let sid = match sid {
        Some(s) => s,
        None => return Ok(()),
    };
    let aid = agent_id()?;

    let stop_reason = payload
        .get("stop_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("end_turn");

    let body = json!({
        "exit_status":         stop_reason,
        "prompt_count":        0,
        "tool_call_count":     0,
        "file_write_count":    0,
        "commit_count":        0,
        "test_run_count":      0,
        "total_input_tokens":  0,
        "total_output_tokens": 0,
        "total_cost_usd":      0.0,
    });

    patch_auth(
        &format!("{}/atheneum/sessions/{}", envoy_url(), sid),
        &aid,
        body,
    )
    .ok();

    // Trigger magellan→Atheneum symbol import for this project.
    let dir = project_dir();
    if let Some(db_path) = magellan_db_path(&dir) {
        let project = project_name(&dir);
        post_auth(
            &format!("{}/atheneum/import-magellan/all", envoy_url()),
            &aid,
            json!({
                "magellan_db_path": db_path,
                "agent_name": AGENT_NAME,
                "project_id": project,
                "limit": 500,
            }),
        )
        .ok();
    }

    Ok(())
}

fn cmd_subagent_end() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).ok();
    let payload: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

    let sid = payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(session_id)
        .filter(|s| !s.is_empty());
    let sid = match sid {
        Some(s) => s,
        None => return Ok(()),
    };
    let aid = agent_id()?;
    let dir = project_dir();

    // Patch session end (same as session-end)
    let stop_reason = payload
        .get("stop_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("end_turn");
    patch_auth(
        &format!("{}/atheneum/sessions/{}", envoy_url(), sid),
        &aid,
        json!({
            "exit_status": stop_reason,
            "prompt_count": 0, "tool_call_count": 0,
            "file_write_count": 0, "commit_count": 0,
            "test_run_count": 0, "total_input_tokens": 0,
            "total_output_tokens": 0, "total_cost_usd": 0.0,
        }),
    )
    .ok();

    // Build compact handover summary from git diff --stat
    let diff_stat = std::process::Command::new("git")
        .args(["-C", &dir, "diff", "--stat", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let files_changed: Vec<String> = diff_stat
        .lines()
        .filter(|l| l.contains('|'))
        .map(|l| l.split('|').next().unwrap_or("").trim().to_string())
        .take(20)
        .collect();

    let summary = truncate(
        &format!("subagent stop_reason={stop_reason} diff:{diff_stat}"),
        300,
    );

    post_auth(
        &format!("{}/atheneum/sessions/{}/handover", envoy_url(), sid),
        &aid,
        json!({
            "summary": summary,
            "files_changed": files_changed,
            "outcome": stop_reason,
        }),
    )
    .ok();

    // === Auto-store discovery for subagent results ===
    // Query subagent session for last tool call + summary
    if let Ok(session_data) = get_session_data(&sid, &aid) {
        if let (Some(tool), Some(tool_summary)) =
            (session_data.last_tool_call, session_data.last_summary)
        {
            // Create discovery target from tool name
            let target = format!("subagent_{}", tool.to_lowercase().replace(' ', "-"));

            // Check duplicate (avoid re-storing on retry)
            let discovery_exists =
                discovery_exists_for_session(&sid, &target, &aid).unwrap_or(false);

            if !discovery_exists {
                let project = project_name(&dir);
                let discovery_payload = json!({
                    "agent": "claude-subagent",
                    "discovery_type": "Location",
                    "target": target,
                    "session_id": sid,
                    "auto_stored": true,
                    "tool_used": tool,
                    "summary": tool_summary,
                    "project": project
                });

                post_auth(
                    &format!("{}/atheneum/discoveries", envoy_url()),
                    &aid,
                    discovery_payload,
                )
                .ok();
            }
        }
    }
    // === END auto-discovery ===

    // Emit file-write events for each changed file.
    let numstat = git_diff_numstat(&dir);
    for (file_path, added, deleted) in &numstat {
        post_auth(
            &format!("{}/atheneum/file-writes", envoy_url()),
            &aid,
            json!({
                "session_id":    &sid,
                "file_path":     file_path,
                "lines_added":   added,
                "lines_deleted": deleted,
                "lines_changed": added + deleted,
                "write_type":    "subagent_edit",
            }),
        )
        .ok();
    }

    // Trigger magellan→Atheneum symbol import for this project.
    if let Some(db_path) = magellan_db_path(&dir) {
        let project = project_name(&dir);
        post_auth(
            &format!("{}/atheneum/import-magellan/all", envoy_url()),
            &aid,
            json!({
                "magellan_db_path": db_path,
                "agent_name": AGENT_NAME,
                "project_id": project,
                "limit": 500,
            }),
        )
        .ok();
    }

    Ok(())
}

fn cmd_user_prompt() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).ok();
    let payload: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

    let sid = payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(session_id)
        .filter(|s| !s.is_empty());
    let sid = match sid {
        Some(s) => s,
        None => return Ok(()),
    };
    let aid = agent_id()?;

    let prompt = payload.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
    if prompt.is_empty() {
        return Ok(());
    }

    post_auth(
        &format!("{}/atheneum/prompts", envoy_url()),
        &aid,
        json!({
            "session_id": sid,
            "role":       "user",
            "sequence":   0,
            "input_hash": ahash_hex(prompt),
        }),
    )
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cmd = std::env::args().nth(1);
    let _ = match cmd.as_deref() {
        Some("session-start") => cmd_session_start(),
        Some("tool-call") => cmd_tool_call(),
        Some("session-end") => cmd_session_end(),
        Some("subagent-end") => cmd_subagent_end(),
        Some("user-prompt") => cmd_user_prompt(),
        Some(other) => {
            eprintln!(
                "envoy-hook: unknown command '{other}'\n\
                 usage: envoy-hook session-start | tool-call | session-end | subagent-end | user-prompt"
            );
            Ok(())
        }
        None => {
            eprintln!(
                "envoy-hook: no command given\n\
                 usage: envoy-hook session-start | tool-call | session-end | subagent-end | user-prompt"
            );
            Ok(())
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_accessed_paths_reads_file_inputs() {
        let payload = json!({
            "tool_input": {
                "file_path": "src/main.rs"
            }
        });

        assert_eq!(
            extract_accessed_paths("Read", &payload),
            vec!["src/main.rs"]
        );
    }

    #[test]
    fn tool_relation_events_emit_accessed_and_modified_edges() {
        let read_payload = json!({
            "tool_input": {
                "file_path": "src/lib.rs"
            }
        });
        let read_events =
            tool_relation_events("session-1", "envoy", "Read", &read_payload, "success");
        assert_eq!(read_events.len(), 1);
        assert_eq!(
            read_events[0]["payload"]["relations"][0]["edge_type"],
            "accessed"
        );

        let edit_payload = json!({
            "tool_input": {
                "file_path": "src/lib.rs"
            }
        });
        let edit_events =
            tool_relation_events("session-1", "envoy", "Edit", &edit_payload, "success");
        assert_eq!(edit_events.len(), 1);
        assert_eq!(
            edit_events[0]["payload"]["relations"][0]["edge_type"],
            "modified"
        );
    }

    #[test]
    fn maybe_test_run_body_detects_cargo_test() {
        let payload = json!({
            "tool_input": {
                "command": "cargo test -p forge-agent --lib"
            }
        });

        let body = maybe_test_run_body(
            "session-2",
            "Bash",
            &payload,
            "success",
            1200,
            Some("tests passed"),
        )
        .expect("test run body");

        assert_eq!(body["test_suite"], "cargo");
        assert_eq!(body["result"], "passed");
        assert_eq!(body["test_command"], "cargo test -p forge-agent --lib");
    }

    /// `project_name` resolves the git toplevel basename so a subdir or
    /// worktree maps to the repo name, not the cwd's parent. Falls back to
    /// the dir basename outside a git worktree.
    #[test]
    fn project_name_uses_git_toplevel_basename() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        // Initialize a real git repo so `git rev-parse --show-toplevel` resolves.
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root)
            .output()
            .expect("git init");
        let repo_name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        // Subdir two levels deep — must still resolve to the repo basename.
        let subdir = root.join("nested/sub");
        std::fs::create_dir_all(&subdir).expect("mkdir");
        assert_eq!(
            project_name(subdir.to_str().unwrap()),
            repo_name,
            "subdir should resolve to git toplevel basename"
        );

        // Non-git temp dir falls back to its own basename.
        let nongit = tempfile::tempdir().expect("tempdir2");
        let nongit_name = nongit
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        assert_eq!(
            project_name(nongit.path().to_str().unwrap()),
            nongit_name,
            "non-git dir should fall back to dir basename"
        );
    }
}
