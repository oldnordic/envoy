use serde::{Deserialize, Serialize};

use crate::error::{EnvoyError, Result};

const KIND_DEPENDENCY: &str = "EnvoyDependency";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDependency {
    pub dependency_id: String,
    pub dependent_agent: String,
    pub blocker_agent: String,
    pub reason: String,
    pub created_at: String,
    pub resolved: bool,
}

/// Stateless store for agent-to-agent dependency tracking.
/// Follows the MessageStore pattern: methods take `&SqliteGraph` as first parameter.
pub struct DependencyStore;

impl Default for DependencyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyStore {
    pub fn new() -> Self {
        Self
    }

    /// Create a dependency: dependent is blocked on blocker.
    pub fn create(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        dependent_agent: String,
        blocker_agent: String,
        reason: String,
    ) -> Result<AgentDependency> {
        use sqlitegraph::GraphEntity;

        if dependent_agent == blocker_agent {
            return Err(EnvoyError::InvalidEntity(
                "cannot depend on self".to_string(),
            ));
        }

        // Check for duplicate
        let existing = graph.find_entities_by_kind(KIND_DEPENDENCY)?;
        for e in &existing {
            let dep = e
                .data
                .get("dependent_agent")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let blk = e
                .data
                .get("blocker_agent")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let res = e
                .data
                .get("resolved")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if dep == dependent_agent && blk == blocker_agent && !res {
                return Err(EnvoyError::DuplicateDependency {
                    dependent: dependent_agent,
                    blocker: blocker_agent,
                });
            }
        }

        let timestamp = chrono::Utc::now().to_rfc3339();
        let entity = GraphEntity {
            id: 0,
            kind: KIND_DEPENDENCY.to_string(),
            name: format!("dep-{}", uuid::Uuid::new_v4()),
            file_path: None,
            data: serde_json::json!({
                "dependent_agent": dependent_agent,
                "blocker_agent": blocker_agent,
                "reason": reason,
                "created_at": timestamp,
                "resolved": false,
            }),
        };
        let id = graph.insert_entity(&entity)?;

        Ok(AgentDependency {
            dependency_id: id.to_string(),
            dependent_agent,
            blocker_agent,
            reason,
            created_at: timestamp,
            resolved: false,
        })
    }

    /// Find all unresolved dependencies where the given agent is the blocker.
    pub fn find_by_blocker(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        blocker_agent: &str,
    ) -> Result<Vec<AgentDependency>> {
        let entities = graph.find_entities_by_kind(KIND_DEPENDENCY)?;
        Ok(entities
            .iter()
            .filter(|e| {
                let blk = e
                    .data
                    .get("blocker_agent")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let res = e
                    .data
                    .get("resolved")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                blk == blocker_agent && !res
            })
            .map(entity_to_dependency)
            .filter_map(|r| r.ok())
            .collect())
    }

    /// Find all unresolved dependencies where the given agent is blocked.
    pub fn find_by_dependent(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        dependent_agent: &str,
    ) -> Result<Vec<AgentDependency>> {
        let entities = graph.find_entities_by_kind(KIND_DEPENDENCY)?;
        Ok(entities
            .iter()
            .filter(|e| {
                let dep = e
                    .data
                    .get("dependent_agent")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let res = e
                    .data
                    .get("resolved")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                dep == dependent_agent && !res
            })
            .map(entity_to_dependency)
            .filter_map(|r| r.ok())
            .collect())
    }

    /// Resolve (close) a dependency by ID.
    pub fn resolve(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        dependency_id: &str,
    ) -> Result<AgentDependency> {
        let id: i64 = dependency_id
            .parse()
            .map_err(|_| EnvoyError::DependencyNotFound(dependency_id.to_string()))?;
        let mut entity = graph
            .get_entity(id)
            .map_err(|_| EnvoyError::DependencyNotFound(dependency_id.to_string()))?;
        if entity.kind != KIND_DEPENDENCY {
            return Err(EnvoyError::DependencyNotFound(dependency_id.to_string()));
        }
        entity.data["resolved"] = serde_json::json!(true);
        graph.update_entity(&entity)?;
        entity_to_dependency(&entity)
    }
}

fn entity_to_dependency(entity: &sqlitegraph::GraphEntity) -> Result<AgentDependency> {
    Ok(AgentDependency {
        dependency_id: entity.id.to_string(),
        dependent_agent: read_str(&entity.data, "dependent_agent"),
        blocker_agent: read_str(&entity.data, "blocker_agent"),
        reason: read_str(&entity.data, "reason"),
        created_at: read_str(&entity.data, "created_at"),
        resolved: entity
            .data
            .get("resolved")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
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
    fn create_and_find_dependency() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = DependencyStore::new();

        let dep = store
            .create(
                graph,
                "agent-a".into(),
                "agent-b".into(),
                "waiting for fix".into(),
            )
            .unwrap();
        assert!(!dep.dependency_id.is_empty());
        assert!(!dep.resolved);

        let by_blocker = store.find_by_blocker(graph, "agent-b").unwrap();
        assert_eq!(by_blocker.len(), 1);
        assert_eq!(by_blocker[0].dependent_agent, "agent-a");

        let by_dependent = store.find_by_dependent(graph, "agent-a").unwrap();
        assert_eq!(by_dependent.len(), 1);
    }

    #[test]
    fn resolve_dependency() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = DependencyStore::new();

        let dep = store
            .create(graph, "agent-a".into(), "agent-b".into(), "waiting".into())
            .unwrap();
        let resolved = store.resolve(graph, &dep.dependency_id).unwrap();
        assert!(resolved.resolved);

        let by_blocker = store.find_by_blocker(graph, "agent-b").unwrap();
        assert!(by_blocker.is_empty());
    }

    #[test]
    fn reject_duplicate_dependency() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = DependencyStore::new();

        store
            .create(graph, "a".into(), "b".into(), "reason".into())
            .unwrap();
        let result = store.create(graph, "a".into(), "b".into(), "reason2".into());
        assert!(result.is_err());
    }

    #[test]
    fn reject_self_dependency() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = DependencyStore::new();

        let result = store.create(graph, "a".into(), "a".into(), "self".into());
        assert!(result.is_err());
    }
}
