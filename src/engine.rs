use sqlitegraph::backend::native::v3::pubsub::{PubSubEvent, Publisher};
use sqlitegraph::pattern_engine::PatternTriple;
use sqlitegraph::{GraphEdge, GraphEntity, SqliteGraph};

use crate::error::{EnvoyError, Result};
use crate::types::{AgentStatus, Channel, EngineStats, Event, EventPayload, Subscription};

const KIND_CHANNEL: &str = "EnvoyChannel";
const KIND_EVENT: &str = "EnvoyEvent";
const KIND_SUBSCRIPTION: &str = "EnvoySubscription";

const EDGE_POSTED_IN: &str = "POSTED_IN";
const EDGE_SUBSCRIBES_TO: &str = "SUBSCRIBES_TO";

/// The envoy coordination engine — wraps sqlitegraph's graph database
/// and pub/sub Publisher for agent-oriented coordination.
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
        if let Ok(_existing) = self.find_channel_entity(name) {
            return Err(EnvoyError::ChannelAlreadyExists(name.to_string()));
        }

        let entity = GraphEntity {
            id: 0,
            kind: KIND_CHANNEL.to_string(),
            name: name.to_string(),
            file_path: None,
            data: serde_json::json!({"description": description}),
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
        })
    }

    pub fn get_channel(&self, name: &str) -> Result<Channel> {
        let entity = self.find_channel_entity(name)?;
        let desc = entity
            .data
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        Ok(Channel {
            id: entity.id,
            name: entity.name.clone(),
            description: desc.to_string(),
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
        Ok(Channel {
            id: entity.id,
            name: entity.name.clone(),
            description: desc.to_string(),
        })
    }

    pub fn list_channels(&self) -> Result<Vec<Channel>> {
        let ids = self.graph.list_entity_ids()?;
        let mut channels = Vec::new();
        for id in ids {
            if let Ok(entity) = self.graph.get_entity(id) {
                if entity.kind == KIND_CHANNEL {
                    let desc = entity
                        .data
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    channels.push(Channel {
                        id: entity.id,
                        name: entity.name.clone(),
                        description: desc.to_string(),
                    });
                }
            }
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

        if let Ok(existing) = self.find_subscription_entity(agent_id, channel.id) {
            let last_seen = existing
                .data
                .get("last_seen_sequence")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            return Ok(Subscription {
                agent_id: agent_id.to_string(),
                channel_id: channel.id,
                channel_name: channel.name,
                last_seen_sequence: last_seen,
            });
        }

        let current_max = self.max_sequence_id(channel.id)?;
        let name = format!("sub-{}-{}", agent_id, channel.id);
        let entity = GraphEntity {
            id: 0,
            kind: KIND_SUBSCRIPTION.to_string(),
            name,
            file_path: None,
            data: serde_json::json!({
                "agent_id": agent_id,
                "channel_id": channel.id,
                "channel_name": channel.name,
                "last_seen_sequence": current_max,
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
        self.graph.insert_edge(&edge)?;

        self.publisher.emit(PubSubEvent::NodeChanged {
            node_id: id,
            snapshot_id: 0,
        });

        Ok(Subscription {
            agent_id: agent_id.to_string(),
            channel_id: channel.id,
            channel_name: channel.name,
            last_seen_sequence: current_max,
        })
    }

    pub fn unsubscribe(&self, agent_id: &str, channel_name: &str) -> Result<()> {
        let channel = self.get_channel(channel_name)?;
        let sub_entity = self.find_subscription_entity(agent_id, channel.id)?;

        let pattern = PatternTriple::new(EDGE_SUBSCRIBES_TO);
        let matches = self.graph.match_triples(&pattern)?;
        for m in matches {
            if m.start_id == sub_entity.id || m.end_id == sub_entity.id {
                let _ = self.graph.delete_edge(m.edge_id);
            }
        }

        self.graph.delete_entity(sub_entity.id)?;
        Ok(())
    }

    pub fn get_subscription(&self, agent_id: &str, channel_name: &str) -> Result<Subscription> {
        let channel = self.get_channel(channel_name)?;
        let entity = self.find_subscription_entity(agent_id, channel.id)?;
        let last_seen = entity
            .data
            .get("last_seen_sequence")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok(Subscription {
            agent_id: agent_id.to_string(),
            channel_id: channel.id,
            channel_name: channel.name,
            last_seen_sequence: last_seen,
        })
    }

    pub fn list_subscriptions(&self, agent_id: &str) -> Result<Vec<Subscription>> {
        let ids = self.graph.list_entity_ids()?;
        let mut subs = Vec::new();
        for id in ids {
            if let Ok(entity) = self.graph.get_entity(id) {
                if entity.kind == KIND_SUBSCRIPTION {
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
                        if let Ok(channel) = self.get_channel_by_id(channel_id) {
                            subs.push(Subscription {
                                agent_id: agent_id.to_string(),
                                channel_id,
                                channel_name: channel.name,
                                last_seen_sequence: last_seen,
                            });
                        }
                    }
                }
            }
        }
        Ok(subs)
    }

    // ── Status ──

    pub fn status(&self) -> Result<EngineStats> {
        let ids = self.graph.list_entity_ids().map_err(EnvoyError::Graph)?;
        let mut channels = 0i64;
        let mut events = 0i64;
        let mut subscriptions = 0i64;
        for id in ids {
            if let Ok(entity) = self.graph.get_entity(id) {
                match entity.kind.as_str() {
                    KIND_CHANNEL => channels += 1,
                    KIND_EVENT => events += 1,
                    KIND_SUBSCRIPTION => subscriptions += 1,
                    _ => {}
                }
            }
        }
        Ok(EngineStats {
            channels,
            events,
            subscriptions,
        })
    }

    // ── Internal helpers ──

    fn find_channel_entity(&self, name: &str) -> Result<GraphEntity> {
        let ids = self.graph.list_entity_ids()?;
        for id in ids {
            if let Ok(entity) = self.graph.get_entity(id) {
                if entity.kind == KIND_CHANNEL && entity.name == name {
                    return Ok(entity);
                }
            }
        }
        Err(EnvoyError::ChannelNotFound(name.to_string()))
    }

    fn find_subscription_entity(&self, agent_id: &str, channel_id: i64) -> Result<GraphEntity> {
        let ids = self.graph.list_entity_ids()?;
        for id in ids {
            if let Ok(entity) = self.graph.get_entity(id) {
                if entity.kind != KIND_SUBSCRIPTION {
                    continue;
                }
                let matches_agent =
                    entity.data.get("agent_id").and_then(|v| v.as_str()) == Some(agent_id);
                let matches_channel =
                    entity.data.get("channel_id").and_then(|v| v.as_i64()) == Some(channel_id);
                if matches_agent && matches_channel {
                    return Ok(entity);
                }
            }
        }
        Err(EnvoyError::NotSubscribed {
            agent: agent_id.to_string(),
            channel: self
                .get_channel_by_id(channel_id)
                .map(|c| c.name)
                .unwrap_or_else(|_| format!("id={channel_id}")),
        })
    }

    fn get_channel_events(&self, channel_id: i64) -> Result<Vec<Event>> {
        let pattern = PatternTriple::new(EDGE_POSTED_IN);
        let matches = self.graph.match_triples(&pattern)?;
        let mut events = Vec::new();
        for m in matches {
            if m.end_id == channel_id {
                if let Ok(entity) = self.graph.get_entity(m.start_id) {
                    if entity.kind == KIND_EVENT {
                        events.push(event_from_entity(&entity)?);
                    }
                }
            }
        }
        Ok(events)
    }

    fn next_sequence_id(&self, channel_id: i64) -> Result<i64> {
        Ok(self.max_sequence_id(channel_id)? + 1)
    }

    fn max_sequence_id(&self, channel_id: i64) -> Result<i64> {
        let events = self.get_channel_events(channel_id)?;
        Ok(events.iter().map(|e| e.sequence_id).max().unwrap_or(0))
    }

    fn update_last_seen(&self, agent_id: &str, channel_name: &str, seq: i64) -> Result<()> {
        let channel = self.get_channel(channel_name)?;
        let mut entity = self.find_subscription_entity(agent_id, channel.id)?;
        entity.data["last_seen_sequence"] = serde_json::json!(seq);
        self.graph.update_entity(&entity)?;
        Ok(())
    }
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
