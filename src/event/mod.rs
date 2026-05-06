pub mod bus;

use serde::{Deserialize, Serialize};

pub const KIND_EVENT: &str = "EnvoyEvent";

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
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ci_status" => Some(Self::CiStatus),
            "hook_result" => Some(Self::HookResult),
            "gate_result" => Some(Self::GateResult),
            "doc_sync" => Some(Self::DocSync),
            "task_handoff" => Some(Self::TaskHandoff),
            "gate_block" => Some(Self::GateBlock),
            "graph_refresh" => Some(Self::GraphRefresh),
            _ => None,
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
    pub timestamp: String,
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
