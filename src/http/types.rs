use serde::{Deserialize, Serialize};

use crate::message::MessageEnvelope;
use crate::message::MessageType;
use crate::message::Part;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub kind: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SendMessageRequest {
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub context_id: Option<String>,
    pub parts: Vec<Part>,
}

#[derive(Debug, Deserialize)]
pub struct PollQuery {
    pub to: String,
    #[serde(default)]
    pub since: Option<i64>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub include: Option<String>,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, Serialize)]
pub struct PollResponse {
    pub messages: Vec<MessageEnvelope>,
    pub latest_sequence: i64,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub uptime_seconds: i64,
    pub agents_online: usize,
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub messages_total: i64,
    pub agents_registered: usize,
}

#[derive(Debug, Deserialize)]
pub struct CreateDependencyRequest {
    pub dependent_agent: String,
    pub blocker_agent: String,
    pub reason: String,
}
