use super::types::{MessageEnvelope, MessageType, Part};
use crate::error::{EnvoyError, Result};

const KIND_MESSAGE: &str = "EnvoyMessage";
const KIND_MSG_SEQ_COUNTER: &str = "EnvoyMsgSeqCounter";

/// Stateless message store. All methods take `&SqliteGraph` for the shared connection.
pub struct MessageStore;

impl Default for MessageStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageStore {
    pub fn new() -> Self {
        Self
    }

    /// Store a message and return a fully-built MessageEnvelope.
    #[allow(clippy::too_many_arguments)]
    pub fn store(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        msg_type: MessageType,
        from: String,
        to: String,
        task_id: Option<String>,
        context_id: Option<String>,
        parts: Vec<Part>,
    ) -> Result<MessageEnvelope> {
        use sqlitegraph::GraphEntity;

        let msg_type_val = serde_json::to_value(&msg_type)?;

        let temp = MessageEnvelope {
            message_id: String::new(),
            msg_type,
            from,
            to,
            task_id,
            context_id,
            timestamp: String::new(),
            sequence_id: 0,
            parts,
        };
        temp.validate()?;

        let timestamp = chrono::Utc::now().to_rfc3339();

        // Per-recipient sequence counter
        let counter_name = format!("msg-seq-{}", temp.to);
        let sequence_id = if let Some(mut entity) =
            graph.find_entity_by_kind_and_name(KIND_MSG_SEQ_COUNTER, &counter_name)?
        {
            let next = entity
                .data
                .get("next")
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            entity.data["next"] = serde_json::json!(next + 1);
            graph.update_entity(&entity)?;
            next
        } else {
            let entity = GraphEntity {
                id: 0,
                kind: KIND_MSG_SEQ_COUNTER.to_string(),
                name: counter_name,
                file_path: None,
                data: serde_json::json!({"next": 2}),
            };
            graph.insert_entity(&entity)?;
            1
        };

        let entity = GraphEntity {
            id: 0,
            kind: KIND_MESSAGE.to_string(),
            name: format!("msg-{}", uuid::Uuid::new_v4()),
            file_path: None,
            data: serde_json::json!({
                "msg_type": msg_type_val,
                "from": temp.from,
                "to": temp.to,
                "task_id": temp.task_id,
                "context_id": temp.context_id,
                "timestamp": timestamp,
                "sequence_id": sequence_id,
                "parts": serde_json::to_value(&temp.parts)?,
            }),
        };
        let id = graph.insert_entity(&entity)?;

        Ok(MessageEnvelope {
            message_id: id.to_string(),
            msg_type: temp.msg_type,
            from: temp.from,
            to: temp.to,
            task_id: temp.task_id,
            context_id: temp.context_id,
            timestamp,
            sequence_id,
            parts: temp.parts,
        })
    }

    /// Mark a message as consumed (ACKed) by an agent.
    pub fn ack(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        message_id: &str,
        agent_id: &str,
    ) -> Result<Vec<String>> {
        let id: i64 = message_id
            .parse()
            .map_err(|_| EnvoyError::MessageNotFound(message_id.to_string()))?;
        let mut entity = graph
            .get_entity(id)
            .map_err(|_| EnvoyError::MessageNotFound(message_id.to_string()))?;
        if entity.kind != KIND_MESSAGE {
            return Err(EnvoyError::MessageNotFound(message_id.to_string()));
        }

        let mut acked: Vec<String> = entity
            .data
            .get("acked_by")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        if !acked.iter().any(|a| a == agent_id) {
            acked.push(agent_id.to_string());
        }

        entity.data["acked_by"] = serde_json::to_value(&acked)?;
        graph.update_entity(&entity)?;
        Ok(acked)
    }

    /// Get messages for a recipient since a given sequence_id.
    /// When `include_acked` is false, only returns messages not yet ACKed by the recipient.
    pub fn poll(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        to: &str,
        since: i64,
        limit: i64,
        include_acked: bool,
    ) -> Result<Vec<MessageEnvelope>> {
        let limit = limit.min(100);
        let entities = graph.find_entities_by_kind(KIND_MESSAGE)?;
        let mut messages: Vec<MessageEnvelope> = entities
            .iter()
            .filter(|e| {
                let msg_to = e.data.get("to").and_then(|v| v.as_str()).unwrap_or("");
                let seq = e
                    .data
                    .get("sequence_id")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                if msg_to != to || seq <= since {
                    return false;
                }
                if include_acked {
                    return true;
                }
                // Filter: only include if recipient hasn't ACKed
                let acked_by: Vec<String> = e
                    .data
                    .get("acked_by")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                !acked_by.iter().any(|a| a == to)
            })
            .map(entity_to_envelope)
            .filter_map(|r| r.ok())
            .collect();
        messages.sort_by_key(|m| m.sequence_id);
        messages.truncate(limit as usize);
        Ok(messages)
    }

    /// Get a single message by ID.
    pub fn get(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        message_id: &str,
    ) -> Result<MessageEnvelope> {
        let id: i64 = message_id
            .parse()
            .map_err(|_| EnvoyError::MessageNotFound(message_id.to_string()))?;
        let entity = graph
            .get_entity(id)
            .map_err(|_| EnvoyError::MessageNotFound(message_id.to_string()))?;
        if entity.kind != KIND_MESSAGE {
            return Err(EnvoyError::MessageNotFound(message_id.to_string()));
        }
        entity_to_envelope(&entity)
    }

    /// Get total message count.
    pub fn count_all(&self, graph: &sqlitegraph::SqliteGraph) -> Result<i64> {
        Ok(graph.find_entities_by_kind(KIND_MESSAGE)?.len() as i64)
    }
}

fn entity_to_envelope(entity: &sqlitegraph::GraphEntity) -> Result<MessageEnvelope> {
    let msg_type: MessageType = entity
        .data
        .get("msg_type")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(MessageType::Direct);
    let parts: Vec<Part> = entity
        .data
        .get("parts")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    Ok(MessageEnvelope {
        message_id: entity.id.to_string(),
        msg_type,
        from: read_json_str(&entity.data, "from"),
        to: read_json_str(&entity.data, "to"),
        task_id: entity
            .data
            .get("task_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        context_id: entity
            .data
            .get("context_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        timestamp: read_json_str(&entity.data, "timestamp"),
        sequence_id: entity
            .data
            .get("sequence_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        parts,
    })
}

fn read_json_str(data: &serde_json::Value, key: &str) -> String {
    data.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::super::types::{MessageType, Part, PartContent};
    use super::*;
    use crate::engine::Engine;

    #[test]
    fn message_store_assigns_ids() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = MessageStore::new();

        let stored = store
            .store(
                graph,
                MessageType::Direct,
                "id1".into(),
                "id2".into(),
                None,
                None,
                vec![Part {
                    content: PartContent::Text("hello".into()),
                }],
            )
            .unwrap();
        assert!(!stored.message_id.is_empty());
        assert!(!stored.timestamp.is_empty());
        assert_eq!(stored.sequence_id, 1);

        let stored2 = store
            .store(
                graph,
                MessageType::Direct,
                "id1".into(),
                "id2".into(),
                None,
                None,
                vec![Part {
                    content: PartContent::Text("world".into()),
                }],
            )
            .unwrap();
        assert_eq!(stored2.sequence_id, 2);

        let msgs = store.poll(graph, "id2", 0, 50, true).unwrap();
        assert_eq!(msgs.len(), 2);

        let msgs = store.poll(graph, "id2", 1, 50, true).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].sequence_id, 2);
    }
}
