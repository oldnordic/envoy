pub mod agents;
pub mod audit;
pub mod circuits;
pub mod diagnostics;
pub mod messages;
pub mod project;
pub mod subscriptions;
pub mod tasks;

// Re-export all pub(crate) items so existing callers (router.rs, etc.)
// continue to work via `use crate::http::handlers::*`.
pub(crate) use agents::{disconnect_agent, get_agent, list_agents, register_agent, retire_agent};
pub(crate) use audit::{
    ingest_ci_event, ingest_doc_event, ingest_gate_event, ingest_hook_event, ingest_verify_event,
    query_audit, query_events, query_task_audit,
};
pub(crate) use circuits::{
    create_dependency, get_blocker_deps, get_circuit, get_dependent_deps, get_nudge_config,
    heartbeat, record_circuit_failure, resolve_dependency, update_nudge_config,
};
pub(crate) use diagnostics::{health, stats};
pub(crate) use messages::{
    ack_message, get_message, pending_messages, poll_messages, send_message,
};
pub(crate) use project::{get_project_config, set_project_config};
pub(crate) use subscriptions::{list_subscriptions, subscribe_agent, unsubscribe_agent};
pub(crate) use tasks::{
    claim_next_task, claim_task, get_task, list_tasks, propose_task, update_task_state,
};
