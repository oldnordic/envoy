use crate::error::Result;
use crate::event::bus::EventBus;
use crate::event::{EnvoyEvent, EventSeverity, EventType};
use crate::message::MessageType;

pub const AUDIT_PROJECT: &str = "_envoy_audit";

/// Lightweight audit logging that reuses the event bus.
/// All audit records are stored as events in the reserved `_envoy_audit` project.
pub struct AuditStore {
    event_bus: EventBus,
}

impl Default for AuditStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditStore {
    pub fn new() -> Self {
        Self {
            event_bus: EventBus::new(),
        }
    }

    /// Log that a message was sent.
    pub fn log_message(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        from: &str,
        to: &str,
        msg_type: MessageType,
        msg_id: &str,
        task_id: Option<&str>,
    ) -> Result<()> {
        let mut data = serde_json::json!({
            "agent_id": from,
            "target": to,
            "msg_type": msg_type.as_str(),
            "msg_id": msg_id,
        });
        if let Some(tid) = task_id {
            data["task_id"] = serde_json::json!(tid);
        }
        let _ = self.event_bus.ingest(
            graph,
            AUDIT_PROJECT.to_string(),
            EventType::AuditLog,
            EventSeverity::Info,
            "message_sent".to_string(),
            format!("Agent {} sent {:?} message to {}", from, msg_type, to),
            data,
        )?;
        Ok(())
    }

    /// Log that an event was ingested.
    pub fn log_event_ingested(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        project: &str,
        source: &str,
        event_type: EventType,
    ) -> Result<()> {
        let _ = self.event_bus.ingest(
            graph,
            AUDIT_PROJECT.to_string(),
            EventType::AuditLog,
            EventSeverity::Info,
            "event_ingested".to_string(),
            format!(
                "Event {} ingested from {} for project {}",
                event_type.as_str(),
                source,
                project
            ),
            serde_json::json!({
                "agent_id": source,
                "target": project,
                "event_type": event_type.as_str(),
            }),
        )?;
        Ok(())
    }

    /// Log agent registration.
    pub fn log_agent_registered(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
        name: &str,
        kind: &str,
    ) -> Result<()> {
        let _ = self.event_bus.ingest(
            graph,
            AUDIT_PROJECT.to_string(),
            EventType::AuditLog,
            EventSeverity::Info,
            "agent_registered".to_string(),
            format!("Agent {} ({}) registered as {}", agent_id, name, kind),
            serde_json::json!({
                "agent_id": agent_id,
                "name": name,
                "kind": kind,
            }),
        )?;
        Ok(())
    }

    /// Log agent disconnection.
    pub fn log_agent_disconnected(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
    ) -> Result<()> {
        let _ = self.event_bus.ingest(
            graph,
            AUDIT_PROJECT.to_string(),
            EventType::AuditLog,
            EventSeverity::Info,
            "agent_disconnected".to_string(),
            format!("Agent {} disconnected", agent_id),
            serde_json::json!({
                "agent_id": agent_id,
            }),
        )?;
        Ok(())
    }

    /// Log circuit breaker opened.
    pub fn log_circuit_opened(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
        failure_count: u32,
    ) -> Result<()> {
        let _ = self.event_bus.ingest(
            graph,
            AUDIT_PROJECT.to_string(),
            EventType::AuditLog,
            EventSeverity::Warning,
            "circuit_opened".to_string(),
            format!(
                "Circuit breaker opened for agent {} after {} failures",
                agent_id, failure_count
            ),
            serde_json::json!({
                "agent_id": agent_id,
                "failure_count": failure_count,
            }),
        )?;
        Ok(())
    }

    /// Log circuit breaker closed.
    pub fn log_circuit_closed(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
    ) -> Result<()> {
        let _ = self.event_bus.ingest(
            graph,
            AUDIT_PROJECT.to_string(),
            EventType::AuditLog,
            EventSeverity::Info,
            "circuit_closed".to_string(),
            format!("Circuit breaker closed for agent {}", agent_id),
            serde_json::json!({
                "agent_id": agent_id,
            }),
        )?;
        Ok(())
    }

    /// Log a task claim.
    pub fn log_task_claimed(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        task_id: &str,
        agent_id: &str,
    ) -> Result<()> {
        let _ = self.event_bus.ingest(
            graph,
            AUDIT_PROJECT.to_string(),
            EventType::AuditLog,
            EventSeverity::Info,
            "task_claimed".to_string(),
            format!("Task {} claimed by agent {}", task_id, agent_id),
            serde_json::json!({
                "task_id": task_id,
                "agent_id": agent_id,
            }),
        )?;
        Ok(())
    }

    /// Query audit records.
    pub fn query(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: Option<&str>,
        operation: Option<&str>,
        task_id: Option<&str>,
        since: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<EnvoyEvent>> {
        let mut events = self.event_bus.query(graph, AUDIT_PROJECT, since, limit)?;

        if let Some(agent_id) = agent_id {
            events.retain(|e| {
                e.data
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id == agent_id)
            });
        }

        if let Some(operation) = operation {
            events.retain(|e| e.source == operation);
        }

        if let Some(task_id) = task_id {
            events.retain(|e| {
                e.data
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id == task_id)
            });
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;

    #[test]
    fn audit_log_message_roundtrips() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let audit = AuditStore::new();

        audit
            .log_message(graph, "id1", "id2", MessageType::Direct, "msg-123", None)
            .unwrap();

        let records = audit
            .query(graph, Some("id1"), None, None, None, None)
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source, "message_sent");
        assert_eq!(
            records[0].data.get("agent_id").unwrap().as_str().unwrap(),
            "id1"
        );
    }

    #[test]
    fn audit_filter_by_operation() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let audit = AuditStore::new();

        audit
            .log_agent_registered(graph, "id1", "claude", "claude")
            .unwrap();
        audit.log_agent_disconnected(graph, "id1").unwrap();

        let registered = audit
            .query(
                graph,
                Some("id1"),
                Some("agent_registered"),
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(registered.len(), 1);

        let all = audit
            .query(graph, Some("id1"), None, None, None, None)
            .unwrap();
        assert_eq!(all.len(), 2);
    }
}
