pub mod store;

use serde::{Deserialize, Serialize};

pub const KIND_TASK: &str = "EnvoyTask";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Proposed,
    Claimed,
    InProgress,
    WaitingReview,
    Done,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Claimed => "claimed",
            Self::InProgress => "in_progress",
            Self::WaitingReview => "waiting_review",
            Self::Done => "done",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "proposed" => Some(Self::Proposed),
            "claimed" => Some(Self::Claimed),
            "in_progress" => Some(Self::InProgress),
            "waiting_review" => Some(Self::WaitingReview),
            "done" => Some(Self::Done),
            _ => None,
        }
    }

    pub fn can_transition_to(&self, next: &TaskState) -> bool {
        matches!(
            (self, next),
            (Self::Proposed, Self::Claimed)
                | (Self::Claimed, Self::InProgress)
                | (Self::Claimed, Self::Proposed)
                | (Self::InProgress, Self::WaitingReview)
                | (Self::InProgress, Self::Proposed)
                | (Self::WaitingReview, Self::Done)
                | (Self::WaitingReview, Self::InProgress)
                | (Self::Done, _)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub project: String,
    pub description: String,
    pub state: TaskState,
    pub claimed_by: Option<String>,
    pub blocked_by: Vec<String>,
    pub checkpoint: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ProposeTaskRequest {
    pub project: String,
    pub description: String,
    #[serde(default)]
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClaimTaskRequest {
    pub agent_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ClaimNextRequest {
    pub agent_id: String,
    pub project: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskStateRequest {
    pub state: String,
    #[serde(default)]
    pub checkpoint: Option<String>,
}
