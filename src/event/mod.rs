pub mod bus;

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const KIND_EVENT: &str = "EnvoyEvent";

impl FromStr for EventType {
    type Err = crate::error::EnvoyError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "ci_status" => Ok(Self::CiStatus),
            "hook_result" => Ok(Self::HookResult),
            "gate_result" => Ok(Self::GateResult),
            "doc_sync" => Ok(Self::DocSync),
            "task_handoff" => Ok(Self::TaskHandoff),
            "gate_block" => Ok(Self::GateBlock),
            "graph_refresh" => Ok(Self::GraphRefresh),
            "task_verify" => Ok(Self::TaskVerify),
            "audit_log" => Ok(Self::AuditLog),
            _ => Err(crate::error::EnvoyError::InvalidMessage(format!(
                "unknown event type: {}",
                s
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    CiStatus,
    HookResult,
    GateResult,
    DocSync,
    TaskHandoff,
    GateBlock,
    GraphRefresh,
    TaskVerify,
    AuditLog,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CiStatus => "ci_status",
            Self::HookResult => "hook_result",
            Self::GateResult => "gate_result",
            Self::DocSync => "doc_sync",
            Self::TaskHandoff => "task_handoff",
            Self::GateBlock => "gate_block",
            Self::GraphRefresh => "graph_refresh",
            Self::TaskVerify => "task_verify",
            Self::AuditLog => "audit_log",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventSeverity {
    Info,
    Warning,
    Blocking,
}

impl EventSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Blocking => "blocking",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvoyEvent {
    pub id: String,
    pub project: String,
    pub event_type: EventType,
    pub severity: EventSeverity,
    pub source: String,
    pub message: String,
    pub data: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct HookEventRequest {
    pub project: String,
    pub hook_name: String,
    pub exit_code: i32,
    pub output: String,
}

#[derive(Debug, Deserialize)]
pub struct GateEventRequest {
    pub project: String,
    pub gates_passed: u32,
    pub gates_total: u32,
    pub failures: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CiEventRequest {
    pub project: String,
    pub run_id: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub head_branch: String,
    pub display_title: String,
}

#[derive(Debug, Deserialize)]
pub struct DocEventRequest {
    pub project: String,
    pub doc_files: Vec<String>,
    pub last_updated_seconds: i64,
}

#[derive(Debug, Deserialize)]
pub struct VerifyEventRequest {
    pub project: String,
    pub agent_id: String,
    pub task_type: String,
    pub claimed_files: Vec<String>,
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<String>,
}
