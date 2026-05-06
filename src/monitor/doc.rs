use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::event::{EventSeverity, EventType};
use crate::http::SharedState;

/// Background doc freshness monitor. Checks git log for documentation activity.
pub async fn run_doc_monitor(
    state: SharedState,
    project: String,
    _repo_path: String,
    interval_secs: u64,
) {
    let last_checked = Arc::new(Mutex::new(chrono::Utc::now().timestamp()));

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;

        let output = Command::new("git")
            .args(["log", "-1", "--format=%ct"])
            .output();

        let stdout = match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => continue,
        };

        let last_commit_ts: i64 = stdout.parse().unwrap_or(0);
        let now = chrono::Utc::now().timestamp();
        let age_seconds = now - last_commit_ts;

        let mut last = last_checked.lock().unwrap();
        if age_seconds > 86400 && *last < now - 86400 {
            *last = now;
            if let Ok(engine) = state.engine.lock() {
                let severity = if age_seconds > 604800 {
                    EventSeverity::Warning
                } else {
                    EventSeverity::Info
                };
                if let Ok(event) = state.event_bus.ingest(
                    engine.graph(),
                    project.clone(),
                    EventType::DocSync,
                    severity,
                    "doc:wiki".into(),
                    format!("Documentation last touched {}s ago", age_seconds),
                    serde_json::json!({
                        "last_updated_seconds": age_seconds,
                    }),
                ) {
                    let event_json = serde_json::to_value(&event).unwrap_or_default();
                    let subs = state
                        .subscription_store
                        .subscribers(engine.graph(), &project)
                        .unwrap_or_default();
                    for agent_id in &subs {
                        let delivered =
                            state
                                .ws_registry
                                .send_json(agent_id, "doc_event", &event_json);
                        if delivered {
                            let _ = state.delivery_tracker.record_delivery(
                                engine.graph(),
                                agent_id,
                                &event.id,
                            );
                        }
                    }
                }
            }
        }
    }
}
