use serde::{Deserialize, Serialize};

/// Agent workflow state, driven by Claude1's complete-agent-workflow checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// Actively executing a task (pre-work → implementation → verification)
    Working,
    /// Blocked on another agent or external dependency
    Blocked,
    /// Work done, waiting for review/CI/docs
    WaitingReview,
    /// No active task
    Idle,
}

impl AgentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::WaitingReview => "waiting_review",
            Self::Idle => "idle",
        }
    }
}

/// A point-in-time snapshot of an agent's status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusSnapshot {
    pub state: AgentState,
    pub task_id: Option<String>,
    pub blocked_reason: Option<String>,
    /// Agent this one is waiting on (if Blocked)
    pub waiting_on_agent: Option<String>,
    /// Workflow checkpoint the agent is currently at
    pub checkpoint: Option<String>,
    /// Human-readable description of current work
    pub working_on: String,
}

impl Default for AgentStatusSnapshot {
    fn default() -> Self {
        Self {
            state: AgentState::Working,
            task_id: None,
            blocked_reason: None,
            waiting_on_agent: None,
            checkpoint: None,
            working_on: "heartbeat via WS".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub agent_id: String,
    pub status: AgentStatusSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub accepted: bool,
    pub nudges: Vec<NudgeMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NudgeMessage {
    pub reason: String,
    pub severity: NudgeSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeSeverity {
    Info,
    Warning,
    Blocking,
}

/// Configuration for the background nudge loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NudgeConfig {
    /// Minutes before an agent without heartbeat is considered stale
    pub stale_threshold_minutes: i64,
    /// Minutes between nudge loop checks
    pub check_interval_seconds: u64,
}

impl Default for NudgeConfig {
    fn default() -> Self {
        Self {
            stale_threshold_minutes: 5,
            check_interval_seconds: 30,
        }
    }
}
