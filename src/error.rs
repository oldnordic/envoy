use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnvoyError {
    #[error("graph error: {0}")]
    Graph(#[from] sqlitegraph::SqliteGraphError),

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
