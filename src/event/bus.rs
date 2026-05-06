use sqlitegraph::GraphEntity;

use crate::error::Result;
use crate::event::{EnvoyEvent, EventSeverity, EventType, KIND_EVENT};

pub struct EventBus;

impl EventBus {
    pub fn new() -> Self {
        Self
    }

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
        let timestamp = chrono::Utc::now().to_rfc3339();
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
                "timestamp": timestamp,
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
        let entities = graph.find_entities_by_kind(KIND_EVENT)?;
        let mut events: Vec<EnvoyEvent> = entities
            .iter()
            .filter(|e| read_str(&e.data, "project") == project)
            .filter(|e| since.map_or(true, |s| read_str(&e.data, "timestamp").as_str() > s))
            .filter_map(|e| entity_to_event(e).ok())
            .collect();
        events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        if let Some(limit) = limit {
            events.truncate(limit as usize);
        }
        Ok(events)
    }

    pub fn purge_old_events(&self, graph: &sqlitegraph::SqliteGraph) -> Result<usize> {
        let cutoff = (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
        let entities = graph.find_entities_by_kind(KIND_EVENT)?;
        let mut purged = 0usize;
        for e in &entities {
            let ts = read_str(&e.data, "timestamp");
            if !ts.is_empty() && ts.as_str() < cutoff.as_str() {
                if graph.delete_entity(e.id).is_ok() {
                    purged += 1;
                }
            }
        }
        Ok(purged)
    }
}

fn entity_to_event(entity: &sqlitegraph::GraphEntity) -> Result<EnvoyEvent> {
    Ok(EnvoyEvent {
        id: entity.id.to_string(),
        project: read_str(&entity.data, "project"),
        event_type: EventType::from_str(&read_str(&entity.data, "event_type"))
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
        timestamp: read_str(&entity.data, "timestamp"),
    })
}

fn read_str(data: &serde_json::Value, key: &str) -> String {
    data.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;

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
