use serde::{Deserialize, Serialize};

use crate::error::{EnvoyError, Result};

/// Content part — exactly one variant per part (adapted from A2A).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartContent {
    Text(String),
    Data(serde_json::Value),
    Url(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    #[serde(flatten)]
    pub content: PartContent,
}

/// Message envelope shared by all message types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub message_id: String,
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub context_id: Option<String>,
    pub timestamp: String,
    pub sequence_id: i64,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Direct,
    Handoff,
    Heartbeat,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompletionStatus {
    Done,
    DoneWithConcerns,
    Blocked,
    NeedsContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatWasDone {
    pub scope: String,
    pub change: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatIsStubbed {
    pub location: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationState {
    pub tests_passing: i64,
    pub tests_failing: i64,
    pub quality_gate: QualityGateResult,
    pub cargo_check_passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityGateResult {
    pub passed: bool,
    pub blocking: i64,
    pub warnings: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MagellanTracePayload {
    pub files_changed: Vec<String>,
    pub symbols_added: Vec<String>,
    pub symbols_removed: Vec<String>,
    #[serde(default)]
    pub refs_in: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub refs_out: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffData {
    pub completion_status: CompletionStatus,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    pub context_remaining_pct: u8,
    pub what_was_done: Vec<WhatWasDone>,
    pub what_is_stubbed: Vec<WhatIsStubbed>,
    pub remaining_work: Vec<String>,
    pub verification_state: VerificationState,
    pub magellan_trace: MagellanTracePayload,
    pub grounded_queries_used: Vec<String>,
}

impl HandoffData {
    pub fn validate(&self) -> Result<()> {
        if self.completion_status == CompletionStatus::Blocked && self.blocked_reason.is_none() {
            return Err(EnvoyError::InvalidMessage(
                "blocked_reason is required when status is BLOCKED".into(),
            ));
        }
        if self.context_remaining_pct > 100 {
            return Err(EnvoyError::InvalidMessage(
                "context_remaining_pct must be 0-100".into(),
            ));
        }
        Ok(())
    }
}

impl MessageEnvelope {
    pub const MAX_BODY_SIZE: usize = 1_048_576; // 1 MB
    pub const MAX_PARTS: usize = 20;

    pub fn validate(&self) -> Result<()> {
        if self.parts.is_empty() {
            return Err(EnvoyError::InvalidMessage(
                "at least one part required".into(),
            ));
        }
        if self.parts.len() > Self::MAX_PARTS {
            return Err(EnvoyError::TooManyParts(self.parts.len()));
        }
        for part in &self.parts {
            if let PartContent::Text(ref text) = &part.content {
                if text.len() > Self::MAX_BODY_SIZE {
                    return Err(EnvoyError::MessageTooLarge(text.len()));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_message_envelope() {
        let msg = MessageEnvelope {
            message_id: "m-001".into(),
            msg_type: MessageType::Direct,
            from: "id1".into(),
            to: "id2".into(),
            task_id: Some("t-001".into()),
            context_id: Some("c-001".into()),
            timestamp: "2026-05-05T21:00:00Z".into(),
            sequence_id: 1,
            parts: vec![
                Part {
                    content: PartContent::Text("hello".into()),
                },
                Part {
                    content: PartContent::Data(serde_json::json!({"status": "working"})),
                },
            ],
        };

        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.message_id, "m-001");
        assert_eq!(back.msg_type, MessageType::Direct);
        assert_eq!(back.parts.len(), 2);
    }

    #[test]
    fn serialize_handoff_data() {
        let handoff = HandoffData {
            completion_status: CompletionStatus::Blocked,
            blocked_reason: Some("need access to sqlitegraph internal API".into()),
            context_remaining_pct: 28,
            what_was_done: vec![WhatWasDone {
                scope: "src/engine.rs".into(),
                change: "added publish()".into(),
                verified: true,
            }],
            what_is_stubbed: vec![WhatIsStubbed {
                location: "src/http.rs".into(),
                reason: "context too low".into(),
            }],
            remaining_work: vec!["Implement HTTP server".into()],
            verification_state: VerificationState {
                tests_passing: 11,
                tests_failing: 0,
                quality_gate: QualityGateResult {
                    passed: true,
                    blocking: 0,
                    warnings: 2,
                },
                cargo_check_passed: true,
            },
            magellan_trace: MagellanTracePayload {
                files_changed: vec!["src/engine.rs".into()],
                symbols_added: vec!["fn publish".into()],
                symbols_removed: vec![],
                refs_in: Default::default(),
                refs_out: Default::default(),
            },
            grounded_queries_used: vec!["magellan find --name Engine".into()],
        };

        let json = serde_json::to_string_pretty(&handoff).unwrap();
        let back: HandoffData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.completion_status, CompletionStatus::Blocked);
        assert_eq!(back.context_remaining_pct, 28);
        assert!(back.validate().is_ok());
    }

    #[test]
    fn handoff_blocked_requires_reason() {
        let handoff = HandoffData {
            completion_status: CompletionStatus::Blocked,
            blocked_reason: None,
            context_remaining_pct: 50,
            what_was_done: vec![],
            what_is_stubbed: vec![],
            remaining_work: vec![],
            verification_state: VerificationState {
                tests_passing: 0,
                tests_failing: 0,
                quality_gate: QualityGateResult {
                    passed: true,
                    blocking: 0,
                    warnings: 0,
                },
                cargo_check_passed: true,
            },
            magellan_trace: MagellanTracePayload {
                files_changed: vec![],
                symbols_added: vec![],
                symbols_removed: vec![],
                refs_in: Default::default(),
                refs_out: Default::default(),
            },
            grounded_queries_used: vec![],
        };
        assert!(handoff.validate().is_err());
    }

    #[test]
    fn message_empty_parts_rejected() {
        let msg = MessageEnvelope {
            message_id: "m-001".into(),
            msg_type: MessageType::Direct,
            from: "id1".into(),
            to: "id2".into(),
            task_id: None,
            context_id: None,
            timestamp: "now".into(),
            sequence_id: 1,
            parts: vec![],
        };
        assert!(msg.validate().is_err());
    }

    #[test]
    fn message_too_many_parts_rejected() {
        let parts: Vec<Part> = (0..25)
            .map(|_| Part {
                content: PartContent::Text("x".into()),
            })
            .collect();
        let msg = MessageEnvelope {
            message_id: "m-001".into(),
            msg_type: MessageType::Direct,
            from: "id1".into(),
            to: "id2".into(),
            task_id: None,
            context_id: None,
            timestamp: "now".into(),
            sequence_id: 1,
            parts,
        };
        assert!(msg.validate().is_err());
    }
}
