use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::event::{EventSeverity, EventType};
use crate::http::SharedState;

/// Background CI poller. Polls gh CLI every `interval_secs` and emits events on state changes.
pub async fn run_ci_monitor(
    state: SharedState,
    project: String,
    owner_repo: String,
    interval_secs: u64,
) {
    let last_state = Arc::new(Mutex::new(HashMap::<String, String>::new()));

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;

        let output = tokio::process::Command::new("gh")
            .args([
                "run",
                "list",
                "--repo",
                &owner_repo,
                "--limit",
                "5",
                "--json",
                "status,conclusion,headBranch,displayTitle,databaseId",
            ])
            .output()
            .await;

        let stdout = match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => continue,
        };

        let runs: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap_or_default();

        // Collect changed runs under lock, then release before async work
        let changed: Vec<(String, String, String, String, String)> = {
            let mut cache = last_state.lock().unwrap_or_else(|e| e.into_inner());
            let mut changes = Vec::new();
            for run in &runs {
                let run_id = run["databaseId"].to_string();
                let conclusion = run["conclusion"]
                    .as_str()
                    .unwrap_or("in_progress")
                    .to_string();
                let status = run["status"].as_str().unwrap_or("").to_string();
                let display_title = run["displayTitle"].as_str().unwrap_or("").to_string();
                let head_branch = run["headBranch"].as_str().unwrap_or("").to_string();

                let prev = cache.get(&run_id).cloned().unwrap_or_default();
                if prev != conclusion {
                    cache.insert(run_id.clone(), conclusion.clone());
                    changes.push((run_id, conclusion, status, display_title, head_branch));
                }
            }
            changes
        };
        // cache lock dropped here — safe to .await

        for (run_id, conclusion, status, display_title, head_branch) in changed {
            let state_fb = state.clone();
            let project_fb = project.clone();
            let project_fb2 = project.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let engine = state_fb.engine.lock().unwrap_or_else(|e| e.into_inner());
                let severity = match conclusion.as_str() {
                    "success" => EventSeverity::Info,
                    "failure" => EventSeverity::Blocking,
                    _ => EventSeverity::Info,
                };
                if let Ok(event) = state_fb.event_bus.ingest(
                    engine.graph(),
                    project_fb,
                    EventType::CiStatus,
                    severity,
                    "ci:github".into(),
                    format!("CI {}: {}", run_id, conclusion),
                    serde_json::json!({
                        "run_id": run_id,
                        "status": status,
                        "conclusion": conclusion,
                        "head_branch": head_branch,
                        "display_title": display_title,
                    }),
                ) {
                    let event_json = serde_json::to_value(&event).unwrap_or_default();
                    let subs = state_fb
                        .subscription_store
                        .subscribers(engine.graph(), &project_fb2)
                        .unwrap_or_default();
                    for agent_id in &subs {
                        let delivered =
                            state_fb
                                .ws_registry
                                .send_json(agent_id, "ci_event", &event_json);
                        if delivered {
                            let _ = state_fb.delivery_tracker.record_delivery(
                                engine.graph(),
                                agent_id,
                                &event.id,
                            );
                        }
                    }
                }
            })
            .await;
        }
    }
}
