use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnvoyError {
    #[error("graph error: {0}")]
    Graph(#[from] sqlitegraph::SqliteGraphError),

    #[error("atheneum error: {0}")]
    Atheneum(#[from] anyhow::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("channel not found: {0}")]
    ChannelNotFound(String),

    #[error("channel already exists: {0}")]
    ChannelAlreadyExists(String),

    #[error("agent not subscribed to channel {channel}")]
    NotSubscribed { agent: String, channel: String },

    #[error("invalid entity: {0}")]
    InvalidEntity(String),

    #[error("agent not found: {0}")]
    AgentNotFound(String),

    #[error("agent offline: {0}")]
    AgentOffline(String),

    #[error("agent already exists: {0}")]
    AgentAlreadyExists(String),

    #[error("message not found: {0}")]
    MessageNotFound(String),

    #[error("invalid message: {0}")]
    InvalidMessage(String),

    #[error("websocket error: {0}")]
    WsError(String),

    #[error("message too large: {0} bytes exceeds 1MB limit")]
    MessageTooLarge(usize),

    #[error("too many parts: {0} exceeds 20 limit")]
    TooManyParts(usize),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("agent status stale: {agent_id} (last heartbeat: {last_heartbeat:?}, threshold: {threshold_minutes}m)")]
    StaleAgent {
        agent_id: String,
        last_heartbeat: Option<String>,
        threshold_minutes: i64,
    },

    #[error("dependency already exists: {dependent} is already waiting on {blocker}")]
    DuplicateDependency { dependent: String, blocker: String },

    #[error("dependency not found: {0}")]
    DependencyNotFound(String),

    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("task already claimed: {0} by {1}")]
    TaskAlreadyClaimed(String, String),

    #[error("invalid task state transition: {task_id} from {from} to {to}")]
    InvalidTaskState {
        task_id: String,
        from: String,
        to: String,
    },

    #[error("not task claimant: agent {agent} does not own task {task_id}")]
    NotTaskClaimant { agent: String, task_id: String },

    #[error("project config not found: {0}")]
    ProjectConfigNotFound(String),

    #[error("subscription not found: {0} for project {1}")]
    SubscriptionNotFound(String, String),
}

impl IntoResponse for EnvoyError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::AgentNotFound(_) => (StatusCode::NOT_FOUND, "AGENT_NOT_FOUND"),
            Self::AgentOffline(_) => (StatusCode::CONFLICT, "AGENT_OFFLINE"),
            Self::AgentAlreadyExists(_) => (StatusCode::CONFLICT, "AGENT_ALREADY_EXISTS"),
            Self::MessageNotFound(_) => (StatusCode::NOT_FOUND, "MESSAGE_NOT_FOUND"),
            Self::ChannelNotFound(_) => (StatusCode::NOT_FOUND, "CHANNEL_NOT_FOUND"),
            Self::InvalidMessage(_) => (StatusCode::BAD_REQUEST, "INVALID_MESSAGE"),
            Self::MessageTooLarge(_) => (StatusCode::BAD_REQUEST, "MESSAGE_TOO_LARGE"),
            Self::TooManyParts(_) => (StatusCode::BAD_REQUEST, "TOO_MANY_PARTS"),
            Self::Serialization(_) => (StatusCode::BAD_REQUEST, "SERIALIZATION_ERROR"),
            Self::StaleAgent { .. } => (StatusCode::OK, "STALE_AGENT"),
            Self::DuplicateDependency { .. } => (StatusCode::CONFLICT, "DUPLICATE_DEPENDENCY"),
            Self::DependencyNotFound(_) => (StatusCode::NOT_FOUND, "DEPENDENCY_NOT_FOUND"),
            Self::TaskNotFound(_) => (StatusCode::NOT_FOUND, "TASK_NOT_FOUND"),
            Self::TaskAlreadyClaimed(_, _) => (StatusCode::CONFLICT, "TASK_ALREADY_CLAIMED"),
            Self::InvalidTaskState { .. } => (StatusCode::CONFLICT, "INVALID_TASK_STATE"),
            Self::NotTaskClaimant { .. } => (StatusCode::FORBIDDEN, "NOT_TASK_CLAIMANT"),
            Self::ProjectConfigNotFound(_) => (StatusCode::NOT_FOUND, "PROJECT_CONFIG_NOT_FOUND"),
            Self::SubscriptionNotFound(_, _) => (StatusCode::NOT_FOUND, "SUBSCRIPTION_NOT_FOUND"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        let body = serde_json::json!({
            "error": {
                "code": code,
                "message": self.to_string()
            }
        });

        (status, axum::Json(body)).into_response()
    }
}

pub type Result<T> = std::result::Result<T, EnvoyError>;
