pub mod ci;
pub mod doc;

use sqlitegraph::GraphEntity;

use serde::{Deserialize, Serialize};

use crate::error::{EnvoyError, Result};

pub const KIND_PROJECT_SUB: &str = "EnvoyProjectSubscription";
pub const KIND_PROJECT_CFG: &str = "EnvoyProjectConfig";

/// Stateless store for project-scoped agent subscriptions.
pub struct SubscriptionStore;

impl Default for SubscriptionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SubscriptionStore {
    pub fn new() -> Self {
        Self
    }

    pub fn subscribe(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
        project: &str,
    ) -> Result<()> {
        let name = format!("sub-{}-{}", agent_id, project);
        if graph
            .find_entity_by_kind_and_name(KIND_PROJECT_SUB, &name)?
            .is_some()
        {
            return Ok(());
        }
        let now = chrono::Utc::now().to_rfc3339();
        let entity = GraphEntity {
            id: 0,
            kind: KIND_PROJECT_SUB.to_string(),
            name,
            file_path: None,
            data: serde_json::json!({
                "agent_id": agent_id,
                "project": project,
                "created_at": now,
            }),
        };
        graph.insert_entity(&entity)?;
        Ok(())
    }

    pub fn unsubscribe(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
        project: &str,
    ) -> Result<()> {
        let name = format!("sub-{}-{}", agent_id, project);
        let entity = graph
            .find_entity_by_kind_and_name(KIND_PROJECT_SUB, &name)?
            .ok_or_else(|| {
                EnvoyError::SubscriptionNotFound(agent_id.to_string(), project.to_string())
            })?;
        graph.delete_entity(entity.id)?;
        Ok(())
    }

    pub fn list(&self, graph: &sqlitegraph::SqliteGraph, agent_id: &str) -> Result<Vec<String>> {
        let entities = graph.find_entities_by_kind(KIND_PROJECT_SUB)?;
        Ok(entities
            .iter()
            .filter(|e| read_str(&e.data, "agent_id") == agent_id)
            .map(|e| read_str(&e.data, "project"))
            .collect())
    }

    pub fn subscribers(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        project: &str,
    ) -> Result<Vec<String>> {
        let entities = graph.find_entities_by_kind(KIND_PROJECT_SUB)?;
        Ok(entities
            .iter()
            .filter(|e| read_str(&e.data, "project") == project)
            .map(|e| read_str(&e.data, "agent_id"))
            .collect())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: String,
    pub ci_poll_seconds: u64,
    pub doc_poll_seconds: u64,
    pub ci_repo_owner: String,
    pub doc_files: Vec<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            project: String::new(),
            ci_poll_seconds: 60,
            doc_poll_seconds: 300,
            ci_repo_owner: String::new(),
            doc_files: vec![],
        }
    }
}

/// Stateless store for per-project polling configuration.
pub struct ProjectConfigStore;

impl Default for ProjectConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectConfigStore {
    pub fn new() -> Self {
        Self
    }

    pub fn get(&self, graph: &sqlitegraph::SqliteGraph, project: &str) -> Result<ProjectConfig> {
        let name = format!("cfg-{}", project);
        let entity = graph
            .find_entity_by_kind_and_name(KIND_PROJECT_CFG, &name)?
            .ok_or_else(|| EnvoyError::ProjectConfigNotFound(project.to_string()))?;
        Ok(ProjectConfig {
            project: read_str(&entity.data, "project"),
            ci_poll_seconds: entity
                .data
                .get("ci_poll_seconds")
                .and_then(|v| v.as_u64())
                .unwrap_or(60),
            doc_poll_seconds: entity
                .data
                .get("doc_poll_seconds")
                .and_then(|v| v.as_u64())
                .unwrap_or(300),
            ci_repo_owner: read_str(&entity.data, "ci_repo_owner"),
            doc_files: read_json_array(&entity.data, "doc_files"),
        })
    }

    pub fn set(&self, graph: &sqlitegraph::SqliteGraph, config: &ProjectConfig) -> Result<()> {
        let name = format!("cfg-{}", config.project);
        let doc_json: Vec<serde_json::Value> = config
            .doc_files
            .iter()
            .map(|s| serde_json::json!(s))
            .collect();
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(mut existing) = graph.find_entity_by_kind_and_name(KIND_PROJECT_CFG, &name)? {
            existing.data["ci_poll_seconds"] = serde_json::json!(config.ci_poll_seconds);
            existing.data["doc_poll_seconds"] = serde_json::json!(config.doc_poll_seconds);
            existing.data["ci_repo_owner"] = serde_json::json!(config.ci_repo_owner);
            existing.data["doc_files"] = serde_json::json!(doc_json);
            existing.data["updated_at"] = serde_json::json!(&now);
            graph.update_entity(&existing)?;
        } else {
            let entity = GraphEntity {
                id: 0,
                kind: KIND_PROJECT_CFG.to_string(),
                name,
                file_path: None,
                data: serde_json::json!({
                    "project": config.project,
                    "ci_poll_seconds": config.ci_poll_seconds,
                    "doc_poll_seconds": config.doc_poll_seconds,
                    "ci_repo_owner": config.ci_repo_owner,
                    "doc_files": doc_json,
                    "created_at": now,
                    "updated_at": now,
                }),
            };
            graph.insert_entity(&entity)?;
        }
        Ok(())
    }
}

fn read_str(data: &serde_json::Value, key: &str) -> String {
    data.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn read_json_array(data: &serde_json::Value, key: &str) -> Vec<String> {
    data.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;

    #[test]
    fn subscribe_and_list() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = SubscriptionStore::new();
        store.subscribe(graph, "agent-1", "magellan").unwrap();
        let subs = store.list(graph, "agent-1").unwrap();
        assert!(subs.contains(&"magellan".to_string()));
        let subscribers = store.subscribers(graph, "magellan").unwrap();
        assert!(subscribers.contains(&"agent-1".to_string()));
    }

    #[test]
    fn unsubscribe() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = SubscriptionStore::new();
        store.subscribe(graph, "agent-1", "magellan").unwrap();
        store.unsubscribe(graph, "agent-1", "magellan").unwrap();
        assert!(store.list(graph, "agent-1").unwrap().is_empty());
    }

    #[test]
    fn project_config_crud() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = ProjectConfigStore::new();
        let cfg = ProjectConfig {
            project: "magellan".into(),
            ci_poll_seconds: 30,
            doc_poll_seconds: 120,
            ci_repo_owner: "oldnordic/magellan".into(),
            doc_files: vec!["CHANGELOG.md".into()],
        };
        store.set(graph, &cfg).unwrap();
        let got = store.get(graph, "magellan").unwrap();
        assert_eq!(got.ci_poll_seconds, 30);
        assert_eq!(got.doc_files, vec!["CHANGELOG.md"]);
    }
}
