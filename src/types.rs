use serde::{Deserialize, Serialize};

/// A communication channel that agents publish events to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: i64,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub created_at: String,
}

/// An immutable event in a channel's append-only log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub channel_id: i64,
    pub channel_name: String,
    pub sender: String,
    pub payload: EventPayload,
    pub timestamp: String,
    pub sequence_id: i64,
}

/// The required fields every coordination message must carry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventPayload {
    pub status: AgentStatus,
    pub working_on: String,
    #[serde(default)]
    pub waiting_for: Option<String>,
    #[serde(default)]
    pub can_start: Option<String>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub magellan_trace: Option<MagellanTrace>,
    #[serde(default)]
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Working,
    Waiting,
    Blocked,
    Done,
}

/// Proof of what code actually changed, verifiable against the live magellan index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MagellanTrace {
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default)]
    pub symbols_added: Vec<String>,
    #[serde(default)]
    pub symbols_removed: Vec<String>,
    #[serde(default)]
    pub db_state: Option<MagellanDbState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MagellanDbState {
    pub schema_version: u32,
    pub symbol_count: u64,
}

/// Subscription info for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub agent_id: String,
    pub channel_id: i64,
    pub channel_name: String,
    pub last_seen_sequence: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// Engine-level statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStats {
    pub channels: i64,
    pub events: i64,
    pub subscriptions: i64,
}
