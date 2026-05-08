use std::collections::HashSet;

use chrono::{DateTime, Utc};
use sqlitegraph::GraphEntity;

use crate::error::Result;
use crate::event::{EnvoyEvent, EventSeverity, EventType, KIND_EVENT};

pub const KIND_DELIVERY: &str = "EnvoyEventDelivery";

/// Tracks per-agent event delivery so offline agents get precise replay.
pub struct DeliveryTracker;

impl Default for DeliveryTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl DeliveryTracker {
    pub fn new() -> Self {
        Self
    }

    /// Record that an event was delivered to an agent.
    pub fn record_delivery(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let name = format!("dlv-{}-{}", agent_id, event_id);
        if graph
            .find_entity_by_kind_and_name(KIND_DELIVERY, &name)?
            .is_some()
        {
            return Ok(());
        }
        let now = chrono::Utc::now().to_rfc3339();
        let entity = GraphEntity {
            id: 0,
            kind: KIND_DELIVERY.to_string(),
            name,
            file_path: None,
            data: serde_json::json!({
                "agent_id": agent_id,
                "event_id": event_id,
                "delivered_at": now,
            }),
        };
        graph.insert_entity(&entity)?;
        Ok(())
    }

    /// Get events not yet delivered to an agent for a project.
    pub fn get_undelivered(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
        project: &str,
        limit: Option<i64>,
    ) -> Result<Vec<EnvoyEvent>> {
        let events = graph.find_entities_by_kind(KIND_EVENT)?;
        let deliveries = graph.find_entities_by_kind(KIND_DELIVERY)?;

        let delivered_ids: HashSet<String> = deliveries
            .iter()
            .filter(|d| read_str(&d.data, "agent_id") == agent_id)
            .map(|d| read_str(&d.data, "event_id"))
            .collect();

        let mut undelivered: Vec<EnvoyEvent> = events
            .iter()
            .filter(|e| read_str(&e.data, "project") == project)
            .filter(|e| !delivered_ids.contains(&e.id.to_string()))
            .filter_map(|e| entity_to_event(e).ok())
            .collect();

        undelivered.sort_by_key(|a| a.timestamp);
        if let Some(limit) = limit {
            undelivered.truncate(limit as usize);
        }
        Ok(undelivered)
    }

    /// Clean up delivery records older than 24h.
    pub fn purge_deliveries(&self, graph: &sqlitegraph::SqliteGraph) -> Result<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
        let deliveries = graph.find_entities_by_kind(KIND_DELIVERY)?;
        let mut purged = 0usize;
        for d in &deliveries {
            let ts = read_str(&d.data, "delivered_at");
            if let Ok(dt) = DateTime::parse_from_rfc3339(&ts) {
                if dt.with_timezone(&Utc) < cutoff {
                    match graph.delete_entity(d.id) {
                        Ok(()) => purged += 1,
                        Err(e) => eprintln!("warn: failed to purge delivery {}: {}", d.id, e),
                    }
                }
            }
        }
        Ok(purged)
    }
}

pub struct EventBus;

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ingest(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        project: String,
        event_type: EventType,
        severity: EventSeverity,
        source: String,
        message: String,
        data: serde_json::Value,
    ) -> Result<EnvoyEvent> {
        let timestamp = chrono::Utc::now();
        let name = format!("evt-{}", uuid::Uuid::new_v4());
        let entity = GraphEntity {
            id: 0,
            kind: KIND_EVENT.to_string(),
            name,
            file_path: None,
            data: serde_json::json!({
                "project": project,
                "event_type": event_type.as_str(),
                "severity": severity.as_str(),
                "source": source,
                "message": message,
                "data": data,
                "timestamp": timestamp.to_rfc3339(),
            }),
        };
        let id = graph.insert_entity(&entity)?;

        Ok(EnvoyEvent {
            id: id.to_string(),
            project,
            event_type,
            severity,
            source,
            message,
            data,
            timestamp,
        })
    }

    pub fn query(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        project: &str,
        since: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<EnvoyEvent>> {
        let since_dt: Option<DateTime<Utc>> = since.and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });
        let entities = graph.find_entities_by_kind(KIND_EVENT)?;
        let mut events: Vec<EnvoyEvent> = entities
            .iter()
            .filter(|e| read_str(&e.data, "project") == project)
            .filter(|e| since_dt.is_none_or(|since| parse_ts(&e.data).is_some_and(|ts| ts > since)))
            .filter_map(|e| entity_to_event(e).ok())
            .collect();
        events.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
        if let Some(limit) = limit {
            events.truncate(limit as usize);
        }
        Ok(events)
    }

    pub fn purge_old_events(&self, graph: &sqlitegraph::SqliteGraph) -> Result<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
        let entities = graph.find_entities_by_kind(KIND_EVENT)?;
        let mut purged = 0usize;
        for e in &entities {
            if parse_ts(&e.data).is_some_and(|ts| ts < cutoff) {
                match graph.delete_entity(e.id) {
                    Ok(()) => purged += 1,
                    Err(err) => eprintln!("warn: failed to purge event {}: {}", e.id, err),
                }
            }
        }
        Ok(purged)
    }
}

fn entity_to_event(entity: &sqlitegraph::GraphEntity) -> Result<EnvoyEvent> {
    let ts_str = read_str(&entity.data, "timestamp");
    let timestamp = DateTime::parse_from_rfc3339(&ts_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    Ok(EnvoyEvent {
        id: entity.id.to_string(),
        project: read_str(&entity.data, "project"),
        event_type: read_str(&entity.data, "event_type")
            .parse()
            .unwrap_or(EventType::HookResult),
        severity: match read_str(&entity.data, "severity").as_str() {
            "warning" => EventSeverity::Warning,
            "blocking" => EventSeverity::Blocking,
            _ => EventSeverity::Info,
        },
        source: read_str(&entity.data, "source"),
        message: read_str(&entity.data, "message"),
        data: entity
            .data
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        timestamp,
    })
}

fn read_str(data: &serde_json::Value, key: &str) -> String {
    data.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn parse_ts(data: &serde_json::Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&read_str(data, "timestamp"))
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;

    #[test]
    fn delivery_tracker_records_and_queries_undelivered() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let bus = EventBus::new();
        let tracker = DeliveryTracker::new();

        let evt = bus
            .ingest(
                graph,
                "magellan".into(),
                EventType::CiStatus,
                EventSeverity::Info,
                "ci".into(),
                "test".into(),
                serde_json::json!({}),
            )
            .unwrap();

        // Before delivery: event should be undelivered
        let undelivered = tracker
            .get_undelivered(graph, "agent-1", "magellan", None)
            .unwrap();
        assert_eq!(undelivered.len(), 1);
        assert_eq!(undelivered[0].id, evt.id);

        // Record delivery
        tracker.record_delivery(graph, "agent-1", &evt.id).unwrap();

        // After delivery: event should not appear
        let undelivered = tracker
            .get_undelivered(graph, "agent-1", "magellan", None)
            .unwrap();
        assert!(undelivered.is_empty());

        // Other agent still hasn't received it
        let undelivered = tracker
            .get_undelivered(graph, "agent-2", "magellan", None)
            .unwrap();
        assert_eq!(undelivered.len(), 1);
    }

    #[test]
    fn delivery_tracker_respects_project_boundary() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let bus = EventBus::new();
        let tracker = DeliveryTracker::new();

        bus.ingest(
            graph,
            "envoy".into(),
            EventType::DocSync,
            EventSeverity::Info,
            "doc".into(),
            "test".into(),
            serde_json::json!({}),
        )
        .unwrap();

        // Agent subscribed to magellan shouldn't see envoy events
        let undelivered = tracker
            .get_undelivered(graph, "agent-1", "magellan", None)
            .unwrap();
        assert!(undelivered.is_empty());
    }

    #[test]
    fn ingest_and_query_events() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let bus = EventBus::new();

        bus.ingest(
            graph,
            "magellan".into(),
            EventType::HookResult,
            EventSeverity::Warning,
            "hook:stub".into(),
            "stub found".into(),
            serde_json::json!({"hook": "stub-check"}),
        )
        .unwrap();
        bus.ingest(
            graph,
            "magellan".into(),
            EventType::CiStatus,
            EventSeverity::Info,
            "ci:github".into(),
            "CI green".into(),
            serde_json::json!({"run_id": "123"}),
        )
        .unwrap();

        let results = bus.query(graph, "magellan", None, None).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn filtered_by_project() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let bus = EventBus::new();

        bus.ingest(
            graph,
            "envoy".into(),
            EventType::DocSync,
            EventSeverity::Info,
            "doc:wiki".into(),
            "updated".into(),
            serde_json::json!({}),
        )
        .unwrap();

        assert!(bus.query(graph, "magellan", None, None).unwrap().is_empty());
        assert_eq!(bus.query(graph, "envoy", None, None).unwrap().len(), 1);
    }

    #[test]
    fn purge_old_events() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let bus = EventBus::new();

        bus.ingest(
            graph,
            "magellan".into(),
            EventType::DocSync,
            EventSeverity::Info,
            "test".into(),
            "test".into(),
            serde_json::json!({}),
        )
        .unwrap();
        // New event should not be purged
        let purged = bus.purge_old_events(graph).unwrap();
        assert_eq!(purged, 0);
    }
}
