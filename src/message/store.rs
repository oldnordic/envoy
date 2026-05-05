use std::sync::Arc;

use super::types::{MessageEnvelope, MessageType, Part};
use crate::error::{EnvoyError, Result};

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

        if msg.message_id.is_empty() {
            msg.message_id = uuid::Uuid::new_v4().to_string();
        }

        msg.timestamp = chrono::Utc::now().to_rfc3339();

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
    use super::super::types::{MessageEnvelope, MessageType, Part, PartContent};
    use super::*;

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

        let msgs = store.poll("id2", 0, 50).unwrap();
        assert_eq!(msgs.len(), 2);

        let msgs = store.poll("id2", 1, 50).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].sequence_id, 2);
    }
}
