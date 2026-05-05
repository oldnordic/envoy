use std::sync::Arc;

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

/// Persists messages in SQLite and assigns sequence IDs.
pub struct MessageStore {
    conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
}

impl MessageStore {
    pub fn new(conn: Arc<std::sync::Mutex<rusqlite::Connection>>) -> Self {
        {
            let c = conn.lock().unwrap();
            c.execute_batch(
                "CREATE TABLE IF NOT EXISTS envoy_messages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    msg_type TEXT NOT NULL,
                    from_agent TEXT NOT NULL,
                    to_agent TEXT NOT NULL,
                    task_id TEXT,
                    context_id TEXT,
                    timestamp TEXT NOT NULL,
                    sequence_id INTEGER NOT NULL,
                    parts_json TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_envoy_messages_to_seq
                    ON envoy_messages(to_agent, sequence_id);",
            )
            .expect("failed to create envoy_messages table");
        }
        Self { conn }
    }

    /// Store a message and assign its message_id, timestamp, and sequence_id.
    pub fn store(&self, mut msg: MessageEnvelope) -> Result<MessageEnvelope> {
        msg.validate()?;

        let conn = self.conn.lock().unwrap();

        // Assign message_id if not set
        if msg.message_id.is_empty() {
            msg.message_id = uuid::Uuid::new_v4().to_string();
        }

        msg.timestamp = chrono::Utc::now().to_rfc3339();

        // Compute next sequence_id for this recipient
        let max_seq: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence_id), 0) FROM envoy_messages WHERE to_agent = ?1",
                [&msg.to],
                |row| row.get(0),
            )
            .unwrap_or(0);
        msg.sequence_id = max_seq + 1;

        let parts_json = serde_json::to_string(&msg.parts)?;

        conn.execute(
            "INSERT INTO envoy_messages (msg_type, from_agent, to_agent, task_id, context_id, timestamp, sequence_id, parts_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                serde_json::to_string(&msg.msg_type)?,
                msg.from,
                msg.to,
                msg.task_id,
                msg.context_id,
                msg.timestamp,
                msg.sequence_id,
                parts_json,
            ],
        )?;

        let id = conn.last_insert_rowid();
        msg.message_id = id.to_string();

        Ok(msg)
    }

    /// Get messages for a recipient since a given sequence_id.
    pub fn poll(&self, to: &str, since: i64, limit: i64) -> Result<Vec<MessageEnvelope>> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.min(100);

        let mut stmt = conn.prepare(
            "SELECT id, msg_type, from_agent, to_agent, task_id, context_id, timestamp, sequence_id, parts_json
             FROM envoy_messages
             WHERE to_agent = ?1 AND sequence_id > ?2
             ORDER BY sequence_id ASC
             LIMIT ?3",
        )?;

        let rows = stmt.query_map(rusqlite::params![to, since, limit], |row| {
            let msg_type_str: String = row.get(1)?;
            let msg_type: MessageType =
                serde_json::from_str(&msg_type_str).unwrap_or(MessageType::Direct);
            let parts_json: String = row.get(8)?;
            let parts: Vec<Part> = serde_json::from_str(&parts_json).unwrap_or_default();

            Ok(MessageEnvelope {
                message_id: row.get::<_, i64>(0)?.to_string(),
                msg_type,
                from: row.get(2)?,
                to: row.get(3)?,
                task_id: row.get(4)?,
                context_id: row.get(5)?,
                timestamp: row.get(6)?,
                sequence_id: row.get(7)?,
                parts,
            })
        })?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        Ok(messages)
    }

    /// Get a single message by ID.
    pub fn get(&self, message_id: &str) -> Result<MessageEnvelope> {
        let id: i64 = message_id
            .parse()
            .map_err(|_| EnvoyError::MessageNotFound(message_id.to_string()))?;
        let conn = self.conn.lock().unwrap();

        let msg = conn
            .query_row(
                "SELECT id, msg_type, from_agent, to_agent, task_id, context_id, timestamp, sequence_id, parts_json
                 FROM envoy_messages WHERE id = ?1",
                [id],
                |row| {
                    let msg_type_str: String = row.get(1)?;
                    let msg_type: MessageType =
                        serde_json::from_str(&msg_type_str).unwrap_or(MessageType::Direct);
                    let parts_json: String = row.get(8)?;
                    let parts: Vec<Part> = serde_json::from_str(&parts_json).unwrap_or_default();

                    Ok(MessageEnvelope {
                        message_id: row.get::<_, i64>(0)?.to_string(),
                        msg_type,
                        from: row.get(2)?,
                        to: row.get(3)?,
                        task_id: row.get(4)?,
                        context_id: row.get(5)?,
                        timestamp: row.get(6)?,
                        sequence_id: row.get(7)?,
                        parts,
                    })
                },
            )
            .map_err(|_| EnvoyError::MessageNotFound(message_id.to_string()))?;

        Ok(msg)
    }

    /// Get total message count.
    pub fn count_all(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM envoy_messages", [], |row| row.get(0))
            .unwrap_or(0);
        Ok(count)
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

    #[test]
    fn message_store_assigns_ids() {
        use std::sync::Arc;
        let conn = Arc::new(std::sync::Mutex::new(
            rusqlite::Connection::open_in_memory().unwrap(),
        ));
        let store = MessageStore::new(conn);

        let msg = MessageEnvelope {
            message_id: String::new(),
            msg_type: MessageType::Direct,
            from: "id1".into(),
            to: "id2".into(),
            task_id: None,
            context_id: None,
            timestamp: String::new(),
            sequence_id: 0,
            parts: vec![Part {
                content: PartContent::Text("hello".into()),
            }],
        };

        let stored = store.store(msg).unwrap();
        assert!(!stored.message_id.is_empty());
        assert!(!stored.timestamp.is_empty());
        assert_eq!(stored.sequence_id, 1);

        let stored2 = store
            .store(MessageEnvelope {
                message_id: String::new(),
                msg_type: MessageType::Direct,
                from: "id1".into(),
                to: "id2".into(),
                task_id: None,
                context_id: None,
                timestamp: String::new(),
                sequence_id: 0,
                parts: vec![Part {
                    content: PartContent::Text("world".into()),
                }],
            })
            .unwrap();
        assert_eq!(stored2.sequence_id, 2);

        // Poll for messages to id2 since sequence 0
        let msgs = store.poll("id2", 0, 50).unwrap();
        assert_eq!(msgs.len(), 2);

        // Poll since sequence 1 — only message 2
        let msgs = store.poll("id2", 1, 50).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].sequence_id, 2);
    }
}
