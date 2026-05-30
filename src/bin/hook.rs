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
        .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

fn project_name(dir: &str) -> String {
    std::path::Path::new(dir)
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

// ── Agent ID caching ─────────────────────────────────────────────────────────

fn agent_id_path() -> PathBuf {
    dirs_next()
        .join("envoy")
        .join("hook-agent-id")
}

fn dirs_next() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local").join("share")
        })
}

/// Returns the cached agent-id, registering a new one with Envoy if needed.
fn agent_id() -> Result<String, Box<dyn std::error::Error>> {
    let path = agent_id_path();

    // Return cached ID if it exists and the agent is still active
    if path.exists() {
        let id = std::fs::read_to_string(&path)?.trim().to_string();
        if !id.is_empty() {
            // Verify the agent is still known to Envoy
            let status = ureq::get(&format!("{}/agents/{}", envoy_url(), id))
                .call()
                .map(|r| r.status())
                .unwrap_or(404);
            if status == 200 {
                return Ok(id);
            }
        }
    }

    // Register fresh
    let resp = ureq::post(&format!("{}/agents", envoy_url()))
        .set("Content-Type", "application/json")
        .send_string(&json!({"name": AGENT_NAME, "kind": AGENT_KIND}).to_string())?;

    let body: Value = serde_json::from_str(&resp.into_string()?)?;
    let id = body
        .get("agent_id")
        .and_then(|v| v.as_str())
        .ok_or("no agent_id in registration response")?
        .to_string();

    // Cache it
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &id)?;

    Ok(id)
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
    let aid = agent_id()?;
    // Use cwd from stdin if available (more reliable than env in some contexts)
    let dir = stdin
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(project_dir);
    let project = project_name(&dir);
    let (branch, head) = git_info(&dir);
    let model = std::env::var("CLAUDE_MODEL").ok();

    let body = json!({
        "session_id": sid,
        "agent":      AGENT_NAME,
        "project":    project,
        "tool":       "claude-code",
        "trigger":    "session_start",
        "model":      model,
        "git_branch": branch,
        "git_head":   head,
    });

    post_auth(&format!("{}/atheneum/sessions", envoy_url()), &aid, body)
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

    let tool_name = payload
        .get("tool_name")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "unknown".to_string());

    let input_summary = summarize_input(&tool_name, &payload);
    let input_hash = ahash_hex(&payload.to_string());
    let latency_ms = payload.get("duration_ms").and_then(|v| v.as_i64()).unwrap_or(0);

    let tool_response = payload.get("tool_response").or(payload.get("tool_result"));
    let (output_summary, output_hash, exit_status) = match tool_response {
        Some(r) => {
            let is_error = r
                .get("type")
                .and_then(|v| v.as_str())
                .map(|t| t == "error" || t == "tool_error")
                .unwrap_or(false)
                || r.get("interrupted").and_then(|v| v.as_bool()).unwrap_or(false);
            (
                Some(summarize_response(&tool_name, r)),
                Some(ahash_hex(&r.to_string())),
                if is_error { "error" } else { "success" },
            )
        }
        None => (None, None, "unknown"),
    };

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

    post_auth(&format!("{}/atheneum/tool-calls", envoy_url()), &aid, body)
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
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cmd = std::env::args().nth(1);
    let _ = match cmd.as_deref() {
        Some("session-start") => cmd_session_start(),
        Some("tool-call") => cmd_tool_call(),
        Some("session-end") => cmd_session_end(),
        Some(other) => {
            eprintln!(
                "envoy-hook: unknown command '{other}'\n\
                 usage: envoy-hook session-start | tool-call | session-end"
            );
            Ok(())
        }
        None => {
            eprintln!(
                "envoy-hook: no command given\n\
                 usage: envoy-hook session-start | tool-call | session-end"
            );
            Ok(())
        }
    };
}
