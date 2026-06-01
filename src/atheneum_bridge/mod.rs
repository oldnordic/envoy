//! Envoy-Atheneum Bridge HTTP endpoints.
//!
//! Split from monolithic `atheneum_bridge.rs` (1774 LOC) into focused modules:
//! - `types` — request/response structs
//! - `utils` — shared helper functions
//! - `discovery` — discovery, handoff, knowledge, search, import handlers
//! - `tasks` — task CRUD + journal handlers
//! - `actions` — action trace handlers
//! - `ontology` — ontology class/property handlers
//! - `sessions` — session, prompt, tool-call, file-write, commit, test-run, events

pub mod actions;
pub mod discovery;
pub mod import;
pub mod ontology;
pub mod sessions;
pub mod tasks;
pub mod types;
pub(crate) mod utils;

pub use types::*;

use axum::Router;
use std::sync::Arc;

use crate::http::AppState;

pub fn add_atheneum_routes(router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
    router
        .route(
            "/atheneum/discoveries",
            axum::routing::post(discovery::post_discovery),
        )
        .route(
            "/atheneum/discoveries",
            axum::routing::get(discovery::get_discoveries),
        )
        .route(
            "/atheneum/handoffs",
            axum::routing::post(discovery::post_handoff),
        )
        .route(
            "/atheneum/handoffs/pending",
            axum::routing::get(discovery::get_pending_handoff),
        )
        .route(
            "/atheneum/handoffs/{id}/claim",
            axum::routing::post(discovery::claim_handoff),
        )
        .route(
            "/atheneum/knowledge",
            axum::routing::get(discovery::get_knowledge),
        )
        .route(
            "/atheneum/search",
            axum::routing::get(discovery::get_search),
        )
        .route(
            "/atheneum/tasks",
            axum::routing::post(tasks::post_task).get(tasks::get_tasks),
        )
        .route(
            "/atheneum/tasks/{id}",
            axum::routing::get(tasks::get_task_details_route),
        )
        .route(
            "/atheneum/tasks/{id}/status",
            axum::routing::patch(tasks::patch_task_status_route),
        )
        .route(
            "/atheneum/tasks/{id}/requirements",
            axum::routing::post(tasks::post_task_requirement),
        )
        .route(
            "/atheneum/tasks/{id}/blockers",
            axum::routing::post(tasks::post_task_blocker),
        )
        .route(
            "/atheneum/journals",
            axum::routing::post(tasks::post_journal),
        )
        .route(
            "/atheneum/actions",
            axum::routing::post(actions::post_action).get(actions::get_actions),
        )
        .route(
            "/atheneum/ontology/classes",
            axum::routing::post(ontology::post_ontology_class).get(ontology::get_ontology_classes),
        )
        .route(
            "/atheneum/ontology/properties",
            axum::routing::post(ontology::post_ontology_property)
                .get(ontology::get_ontology_properties),
        )
        .route(
            "/atheneum/ontology/validate",
            axum::routing::get(ontology::get_ontology_validate),
        )
        .route(
            "/atheneum/ontology/seed",
            axum::routing::post(ontology::post_ontology_seed),
        )
        .route(
            "/atheneum/import-magellan/symbol",
            axum::routing::post(import::post_import_magellan_symbol),
        )
        .route(
            "/atheneum/import-magellan/all",
            axum::routing::post(import::post_import_magellan_all),
        )
        .route(
            "/atheneum/sessions",
            axum::routing::post(sessions::post_session).get(sessions::get_sessions),
        )
        .route(
            "/atheneum/sessions/{id}",
            axum::routing::patch(sessions::patch_session),
        )
        .route(
            "/atheneum/sessions/{id}/handover",
            axum::routing::post(sessions::post_subagent_handover),
        )
        .route(
            "/atheneum/prompts",
            axum::routing::post(sessions::post_prompt),
        )
        .route(
            "/atheneum/tool-calls",
            axum::routing::post(sessions::post_tool_call),
        )
        .route(
            "/atheneum/file-writes",
            axum::routing::post(sessions::post_file_write),
        )
        .route(
            "/atheneum/commits",
            axum::routing::post(sessions::post_commit),
        )
        .route(
            "/atheneum/test-runs",
            axum::routing::post(sessions::post_test_run),
        )
        .route(
            "/atheneum/fix-chains",
            axum::routing::post(sessions::post_fix_chain),
        )
        .route(
            "/atheneum/bench-runs",
            axum::routing::post(sessions::post_bench_run),
        )
        .route("/atheneum/events", axum::routing::get(sessions::get_events))
}
