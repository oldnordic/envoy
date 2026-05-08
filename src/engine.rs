use sqlitegraph::backend::native::v3::pubsub::{PubSubEvent, Publisher};
use sqlitegraph::{GraphEdge, GraphEntity, SqliteGraph};

use crate::error::{EnvoyError, Result};
use crate::types::{AgentStatus, Channel, EngineStats, Event, EventPayload, Subscription};

const KIND_CHANNEL: &str = "EnvoyChannel";
const KIND_EVENT: &str = "EnvoyEvent";
const KIND_SUBSCRIPTION: &str = "EnvoySubscription";
const KIND_SEQ_COUNTER: &str = "EnvoySeqCounter";

const EDGE_POSTED_IN: &str = "POSTED_IN";
const EDGE_SUBSCRIBES_TO: &str = "SUBSCRIBES_TO";

/// The envoy coordination engine — wraps sqlitegraph's graph database
/// and pub/sub Publisher for agent-oriented coordination.
///
/// Uses sqlitegraph 2.2.0's indexed kind-based lookups (`find_entity_by_kind_and_name`,
/// `find_entities_by_kind`) for O(1) entity resolution without in-memory caches.
pub struct Engine {
    graph: SqliteGraph,
    publisher: Publisher,
}

impl Engine {
    /// Open (or create) an envoy database backed by sqlitegraph.
    pub fn open(path: &str) -> Result<Self> {
        let graph = SqliteGraph::open(path)?;
        let publisher = Publisher::new();
        Ok(Self { graph, publisher })
    }

    /// Open an in-memory engine for testing.
    pub fn open_in_memory() -> Result<Self> {
        let graph = SqliteGraph::open_in_memory()?;
        let publisher = Publisher::new();
        Ok(Self { graph, publisher })
    }

    /// Access the underlying sqlitegraph Publisher for real-time event listeners.
    pub fn publisher(&self) -> &Publisher {
        &self.publisher
    }

    /// Access the underlying sqlitegraph for direct graph operations.
    pub fn graph(&self) -> &SqliteGraph {
        &self.graph
    }

    // ── Channels ──

    pub fn create_channel(&self, name: &str, description: &str) -> Result<Channel> {
        if self
            .graph
            .find_entity_by_kind_and_name(KIND_CHANNEL, name)?
            .is_some()
        {
            return Err(EnvoyError::ChannelAlreadyExists(name.to_string()));
        }

        let now = chrono::Utc::now().to_rfc3339();
        let entity = GraphEntity {
            id: 0,
            kind: KIND_CHANNEL.to_string(),
            name: name.to_string(),
            file_path: None,
            data: serde_json::json!({"description": description, "created_at": &now}),
        };
        let id = self.graph.insert_entity(&entity)?;

        self.publisher.emit(PubSubEvent::NodeChanged {
            node_id: id,
            snapshot_id: 0,
        });

        Ok(Channel {
            id,
            name: name.to_string(),
            description: description.to_string(),
            created_at: now,
        })
    }

    pub fn get_channel(&self, name: &str) -> Result<Channel> {
        let entity = self
            .graph
            .find_entity_by_kind_and_name(KIND_CHANNEL, name)?
            .ok_or_else(|| EnvoyError::ChannelNotFound(name.to_string()))?;
        let desc = entity
            .data
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let created_at = entity
            .data
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        Ok(Channel {
            id: entity.id,
            name: entity.name.clone(),
            description: desc.to_string(),
            created_at: created_at.to_string(),
        })
    }

    pub fn get_channel_by_id(&self, id: i64) -> Result<Channel> {
        let entity = self
            .graph
            .get_entity(id)
            .map_err(|_| EnvoyError::ChannelNotFound(format!("id={id}")))?;
        if entity.kind != KIND_CHANNEL {
            return Err(EnvoyError::ChannelNotFound(format!("id={id}")));
        }
        let desc = entity
            .data
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let created_at = entity
            .data
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        Ok(Channel {
            id: entity.id,
            name: entity.name.clone(),
            description: desc.to_string(),
            created_at: created_at.to_string(),
        })
    }

    pub fn list_channels(&self) -> Result<Vec<Channel>> {
        let entities = self.graph.find_entities_by_kind(KIND_CHANNEL)?;
        let mut channels = Vec::new();
        for entity in entities {
            let desc = entity
                .data
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let created_at = entity
                .data
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            channels.push(Channel {
                id: entity.id,
                name: entity.name.clone(),
                description: desc.to_string(),
                created_at: created_at.to_string(),
            });
        }
        Ok(channels)
    }

    // ── Publishing ──

    pub fn publish(
        &self,
        channel_name: &str,
        sender: &str,
        payload: EventPayload,
    ) -> Result<Event> {
        let channel = self.get_channel(channel_name)?;
        let now = chrono::Utc::now().to_rfc3339();
        let next_seq = self.next_sequence_id(channel.id)?;

        let name = format!("event-{}-{}", channel.id, next_seq);
        let entity = GraphEntity {
            id: 0,
            kind: KIND_EVENT.to_string(),
            name,
            file_path: None,
            data: serde_json::json!({
                "channel_id": channel.id,
                "channel_name": channel.name,
                "sender": sender,
                "payload": serde_json::to_value(&payload)?,
                "timestamp": now,
                "sequence_id": next_seq,
            }),
        };
        let id = self.graph.insert_entity(&entity)?;

        let edge = GraphEdge {
            id: 0,
            from_id: id,
            to_id: channel.id,
            edge_type: EDGE_POSTED_IN.to_string(),
            data: serde_json::json!({}),
        };
        self.graph.insert_edge(&edge)?;

        self.publisher.emit(PubSubEvent::NodeChanged {
            node_id: id,
            snapshot_id: 0,
        });

        Ok(Event {
            id,
            channel_id: channel.id,
            channel_name: channel.name,
            sender: sender.to_string(),
            payload,
            timestamp: now,
            sequence_id: next_seq,
        })
    }

    pub fn replay(
        &self,
        channel_name: &str,
        since_sequence: i64,
        limit: Option<i64>,
    ) -> Result<Vec<Event>> {
        let channel = self.get_channel(channel_name)?;
        let all_events = self.get_channel_events(channel.id)?;
        let mut events: Vec<Event> = all_events
            .into_iter()
            .filter(|e| e.sequence_id > since_sequence)
            .collect();
        events.sort_by_key(|e| e.sequence_id);
        if let Some(limit) = limit {
            events.truncate(limit as usize);
        }
        Ok(events)
    }

    pub fn catch_up(&self, agent_id: &str, channel_name: &str) -> Result<Vec<Event>> {
        let sub = self.get_subscription(agent_id, channel_name)?;
        let events = self.replay(channel_name, sub.last_seen_sequence, None)?;
        if let Some(last) = events.last() {
            self.update_last_seen(agent_id, channel_name, last.sequence_id)?;
        }
        Ok(events)
    }

    // ── Subscriptions ──

    pub fn subscribe(&self, agent_id: &str, channel_name: &str) -> Result<Subscription> {
        let channel = self.get_channel(channel_name)?;

        // Check for existing subscription via indexed kind+name lookup
        let sub_name = sub_entity_name(agent_id, channel.id);
        if let Some(existing) = self
            .graph
            .find_entity_by_kind_and_name(KIND_SUBSCRIPTION, &sub_name)?
        {
            let last_seen = existing
                .data
                .get("last_seen_sequence")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let created_at = read_json_str(&existing.data, "created_at");
            let updated_at = read_json_str(&existing.data, "updated_at");
            return Ok(Subscription {
                agent_id: agent_id.to_string(),
                channel_id: channel.id,
                channel_name: channel.name,
                last_seen_sequence: last_seen,
                created_at,
                updated_at,
            });
        }

        let current_max = self.max_sequence_id(channel.id)?;
        let now = chrono::Utc::now().to_rfc3339();
        let entity = GraphEntity {
            id: 0,
            kind: KIND_SUBSCRIPTION.to_string(),
            name: sub_name,
            file_path: None,
            data: serde_json::json!({
                "agent_id": agent_id,
                "channel_id": channel.id,
                "channel_name": channel.name,
                "last_seen_sequence": current_max,
                "created_at": &now,
                "updated_at": &now,
            }),
        };
        let id = self.graph.insert_entity(&entity)?;

        let edge = GraphEdge {
            id: 0,
            from_id: id,
            to_id: channel.id,
            edge_type: EDGE_SUBSCRIBES_TO.to_string(),
            data: serde_json::json!({}),
        };
        let edge_id = self.graph.insert_edge(&edge)?;

        // Store edge_id in entity data for O(1) deletion on unsubscribe
        let mut sub_entity = self
            .graph
            .get_entity(id)
            .map_err(|_| EnvoyError::InvalidEntity("subscription not found after insert".into()))?;
        sub_entity.data["sub_edge_id"] = serde_json::json!(edge_id);
        self.graph.update_entity(&sub_entity)?;

        self.publisher.emit(PubSubEvent::NodeChanged {
            node_id: id,
            snapshot_id: 0,
        });

        Ok(Subscription {
            agent_id: agent_id.to_string(),
            channel_id: channel.id,
            channel_name: channel.name,
            last_seen_sequence: current_max,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn unsubscribe(&self, agent_id: &str, channel_name: &str) -> Result<()> {
        let channel = self.get_channel(channel_name)?;
        let sub_name = sub_entity_name(agent_id, channel.id);
        let sub_entity = self
            .graph
            .find_entity_by_kind_and_name(KIND_SUBSCRIPTION, &sub_name)?
            .ok_or_else(|| EnvoyError::NotSubscribed {
                agent: agent_id.to_string(),
                channel: channel.name.clone(),
            })?;

        // O(1): delete edge by stored ID instead of scanning all SUBSCRIBES_TO edges
        if let Some(edge_id) = sub_entity.data.get("sub_edge_id").and_then(|v| v.as_i64()) {
            if let Err(e) = self.graph.delete_edge(edge_id) {
                eprintln!(
                    "warn: failed to delete subscription edge {}: {}",
                    edge_id, e
                );
            }
        }

        self.graph.delete_entity(sub_entity.id)?;
        Ok(())
    }

    pub fn get_subscription(&self, agent_id: &str, channel_name: &str) -> Result<Subscription> {
        let channel = self.get_channel(channel_name)?;
        let sub_name = sub_entity_name(agent_id, channel.id);
        let entity = self
            .graph
            .find_entity_by_kind_and_name(KIND_SUBSCRIPTION, &sub_name)?
            .ok_or_else(|| EnvoyError::NotSubscribed {
                agent: agent_id.to_string(),
                channel: channel.name.clone(),
            })?;
        let last_seen = entity
            .data
            .get("last_seen_sequence")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let created_at = read_json_str(&entity.data, "created_at");
        let updated_at = read_json_str(&entity.data, "updated_at");
        Ok(Subscription {
            agent_id: agent_id.to_string(),
            channel_id: channel.id,
            channel_name: channel.name,
            last_seen_sequence: last_seen,
            created_at,
            updated_at,
        })
    }

    pub fn list_subscriptions(&self, agent_id: &str) -> Result<Vec<Subscription>> {
        let entities = self.graph.find_entities_by_kind(KIND_SUBSCRIPTION)?;
        let mut subs = Vec::new();
        for entity in entities {
            let data_agent = entity
                .data
                .get("agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if data_agent == agent_id {
                let channel_id = entity
                    .data
                    .get("channel_id")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let last_seen = entity
                    .data
                    .get("last_seen_sequence")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let created_at = read_json_str(&entity.data, "created_at");
                let updated_at = read_json_str(&entity.data, "updated_at");
                if let Ok(channel) = self.get_channel_by_id(channel_id) {
                    subs.push(Subscription {
                        agent_id: agent_id.to_string(),
                        channel_id,
                        channel_name: channel.name,
                        last_seen_sequence: last_seen,
                        created_at,
                        updated_at,
                    });
                }
            }
        }
        Ok(subs)
    }

    // ── Status ──

    pub fn status(&self) -> Result<EngineStats> {
        let channels = self.graph.find_entities_by_kind(KIND_CHANNEL)?.len() as i64;
        let events = self.graph.find_entities_by_kind(KIND_EVENT)?.len() as i64;
        let subscriptions = self.graph.find_entities_by_kind(KIND_SUBSCRIPTION)?.len() as i64;
        Ok(EngineStats {
            channels,
            events,
            subscriptions,
        })
    }

    // ── Internal helpers ──

    fn get_channel_events(&self, channel_id: i64) -> Result<Vec<Event>> {
        let entities = self.graph.find_entities_by_kind(KIND_EVENT)?;
        let mut events = Vec::new();
        for entity in entities {
            let evt_channel_id = entity
                .data
                .get("channel_id")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if evt_channel_id == channel_id {
                events.push(event_from_entity(&entity)?);
            }
        }
        events.sort_by_key(|e| e.sequence_id);
        Ok(events)
    }

    fn next_sequence_id(&self, channel_id: i64) -> Result<i64> {
        let counter_name = seq_counter_name(channel_id);
        if let Some(mut entity) = self
            .graph
            .find_entity_by_kind_and_name(KIND_SEQ_COUNTER, &counter_name)?
        {
            let next = entity
                .data
                .get("next")
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            entity.data["next"] = serde_json::json!(next + 1);
            self.graph.update_entity(&entity)?;
            Ok(next)
        } else {
            let entity = GraphEntity {
                id: 0,
                kind: KIND_SEQ_COUNTER.to_string(),
                name: counter_name,
                file_path: None,
                data: serde_json::json!({"next": 2}),
            };
            self.graph.insert_entity(&entity)?;
            Ok(1)
        }
    }

    fn max_sequence_id(&self, channel_id: i64) -> Result<i64> {
        let counter_name = seq_counter_name(channel_id);
        if let Some(entity) = self
            .graph
            .find_entity_by_kind_and_name(KIND_SEQ_COUNTER, &counter_name)?
        {
            let next = entity
                .data
                .get("next")
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            Ok(next - 1)
        } else {
            Ok(0)
        }
    }

    fn update_last_seen(&self, agent_id: &str, channel_name: &str, seq: i64) -> Result<()> {
        let channel = self.get_channel(channel_name)?;
        let sub_name = sub_entity_name(agent_id, channel.id);
        let mut entity = self
            .graph
            .find_entity_by_kind_and_name(KIND_SUBSCRIPTION, &sub_name)?
            .ok_or_else(|| EnvoyError::NotSubscribed {
                agent: agent_id.to_string(),
                channel: channel.name.clone(),
            })?;
        entity.data["last_seen_sequence"] = serde_json::json!(seq);
        entity.data["updated_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        self.graph.update_entity(&entity)?;
        Ok(())
    }
}

fn sub_entity_name(agent_id: &str, channel_id: i64) -> String {
    format!("sub-{}-{}", agent_id, channel_id)
}

fn seq_counter_name(channel_id: i64) -> String {
    format!("seq-{channel_id}")
}

fn read_json_str(data: &serde_json::Value, key: &str) -> String {
    data.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn event_from_entity(entity: &GraphEntity) -> Result<Event> {
    let channel_id = entity
        .data
        .get("channel_id")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let channel_name = entity
        .data
        .get("channel_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let sender = entity
        .data
        .get("sender")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let timestamp = entity
        .data
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let sequence_id = entity
        .data
        .get("sequence_id")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let payload = entity
        .data
        .get("payload")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| EventPayload {
            status: AgentStatus::Working,
            working_on: "unknown".into(),
            waiting_for: None,
            can_start: None,
            verified: false,
            magellan_trace: None,
            extra: serde_json::Value::Null,
        });

    Ok(Event {
        id: entity.id,
        channel_id,
        channel_name: channel_name.to_string(),
        sender: sender.to_string(),
        payload,
        timestamp: timestamp.to_string(),
        sequence_id,
    })
}
