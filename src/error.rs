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
}

pub type Result<T> = std::result::Result<T, EnvoyError>;
