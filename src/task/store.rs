use sqlitegraph::GraphEntity;

use crate::error::{EnvoyError, Result};
use crate::task::{Task, TaskState, KIND_TASK};

pub struct TaskStore;

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskStore {
    pub fn new() -> Self {
        Self
    }

    pub fn propose(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        project: String,
        description: String,
        blocked_by: Vec<String>,
    ) -> Result<Task> {
        let now = chrono::Utc::now().to_rfc3339();
        let name = format!("task-{}", uuid::Uuid::new_v4());
        let blocked_json: Vec<serde_json::Value> =
            blocked_by.iter().map(|s| serde_json::json!(s)).collect();
        let entity = GraphEntity {
            id: 0,
            kind: KIND_TASK.to_string(),
            name,
            file_path: None,
            data: serde_json::json!({
                "project": project,
                "description": description,
                "state": "proposed",
                "claimed_by": null,
                "blocked_by": blocked_json,
                "checkpoint": null,
                "created_at": now,
                "updated_at": now,
            }),
        };
        let id = graph.insert_entity(&entity)?;
        Ok(Task {
            id: id.to_string(),
            project,
            description,
            state: TaskState::Proposed,
            claimed_by: None,
            blocked_by,
            checkpoint: None,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn claim(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        task_id: &str,
        agent_id: String,
    ) -> Result<Task> {
        let task = self.get(graph, task_id)?;
        if task.state != TaskState::Proposed {
            return Err(EnvoyError::TaskAlreadyClaimed(
                task_id.to_string(),
                task.claimed_by.unwrap_or_default(),
            ));
        }
        let now = chrono::Utc::now().to_rfc3339();
        self.update_entity(graph, task_id, |data| {
            data["state"] = serde_json::json!("claimed");
            data["claimed_by"] = serde_json::json!(&agent_id);
            data["updated_at"] = serde_json::json!(&now);
            data["checkpoint"] = serde_json::json!("claimed");
        })?;
        Ok(Task {
            state: TaskState::Claimed,
            claimed_by: Some(agent_id),
            updated_at: now,
            ..task
        })
    }

    pub fn claim_next(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        project: &str,
        agent_id: String,
    ) -> Result<Task> {
        let tasks = self.list(graph, project, Some(&TaskState::Proposed))?;
        if tasks.is_empty() {
            return Err(EnvoyError::TaskNotFound("no proposed tasks".into()));
        }
        let oldest = tasks
            .into_iter()
            .min_by_key(|t| t.created_at.clone())
            .ok_or_else(|| EnvoyError::TaskNotFound("no proposed tasks after filter".into()))?;
        self.claim(graph, &oldest.id, agent_id)
    }

    pub fn update_state(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        task_id: &str,
        new_state: TaskState,
        checkpoint: Option<String>,
        agent_id: Option<&str>,
    ) -> Result<Task> {
        let task = self.get(graph, task_id)?;
        if !task.state.can_transition_to(&new_state) {
            return Err(EnvoyError::InvalidTaskState {
                task_id: task_id.to_string(),
                from: task.state.as_str().to_string(),
                to: new_state.as_str().to_string(),
            });
        }
        if let (Some(ref claimant), Some(agent)) = (&task.claimed_by, agent_id) {
            if claimant != agent && new_state != TaskState::Proposed {
                return Err(EnvoyError::NotTaskClaimant {
                    agent: agent.to_string(),
                    task_id: task_id.to_string(),
                });
            }
        }
        let now = chrono::Utc::now().to_rfc3339();
        self.update_entity(graph, task_id, |data| {
            data["state"] = serde_json::json!(new_state.as_str());
            data["updated_at"] = serde_json::json!(&now);
            if let Some(ref cp) = checkpoint {
                data["checkpoint"] = serde_json::json!(cp);
            }
        })?;
        Ok(Task {
            state: new_state,
            checkpoint: checkpoint.or(task.checkpoint),
            updated_at: now,
            ..task
        })
    }

    pub fn get(&self, graph: &sqlitegraph::SqliteGraph, task_id: &str) -> Result<Task> {
        let id: i64 = task_id
            .parse()
            .map_err(|_| EnvoyError::TaskNotFound(task_id.to_string()))?;
        let entity = graph
            .get_entity(id)
            .map_err(|_| EnvoyError::TaskNotFound(task_id.to_string()))?;
        if entity.kind != KIND_TASK {
            return Err(EnvoyError::TaskNotFound(task_id.to_string()));
        }
        entity_to_task(&entity)
    }

    pub fn list(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        project: &str,
        state_filter: Option<&TaskState>,
    ) -> Result<Vec<Task>> {
        let entities = graph.find_entities_by_kind(KIND_TASK)?;
        let mut tasks: Vec<Task> = entities
            .iter()
            .filter(|e| read_str(&e.data, "project") == project)
            .filter_map(|e| entity_to_task(e).ok())
            .filter(|t| state_filter.is_none_or(|f| t.state == *f))
            .collect();
        tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(tasks)
    }

    pub fn find_blocked_by(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        blocker_id: &str,
    ) -> Result<Vec<Task>> {
        let entities = graph.find_entities_by_kind(KIND_TASK)?;
        Ok(entities
            .iter()
            .filter(|e| {
                read_json_array(&e.data, "blocked_by")
                    .iter()
                    .any(|b| b == blocker_id)
            })
            .filter_map(|e| entity_to_task(e).ok())
            .collect())
    }

    pub fn reclaim_stale(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        stale_agent_id: &str,
    ) -> Result<Vec<String>> {
        let entities = graph.find_entities_by_kind(KIND_TASK)?;
        let mut reclaimed = Vec::new();
        for e in &entities {
            let claimed_by = read_str(&e.data, "claimed_by");
            let state = read_str(&e.data, "state");
            if claimed_by == stale_agent_id && (state == "claimed" || state == "in_progress") {
                let now = chrono::Utc::now().to_rfc3339();
                let mut entity = e.clone();
                entity.data["state"] = serde_json::json!("proposed");
                entity.data["claimed_by"] = serde_json::json!(null);
                entity.data["checkpoint"] = serde_json::json!(null);
                entity.data["updated_at"] = serde_json::json!(&now);
                graph.update_entity(&entity)?;
                reclaimed.push(entity.id.to_string());
            }
        }
        Ok(reclaimed)
    }

    fn update_entity(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        task_id: &str,
        updater: impl FnOnce(&mut serde_json::Value),
    ) -> Result<()> {
        let id: i64 = task_id
            .parse()
            .map_err(|_| EnvoyError::TaskNotFound(task_id.to_string()))?;
        let mut entity = graph
            .get_entity(id)
            .map_err(|_| EnvoyError::TaskNotFound(task_id.to_string()))?;
        if entity.kind != KIND_TASK {
            return Err(EnvoyError::TaskNotFound(task_id.to_string()));
        }
        updater(&mut entity.data);
        graph.update_entity(&entity)?;
        Ok(())
    }
}

fn entity_to_task(entity: &sqlitegraph::GraphEntity) -> Result<Task> {
    Ok(Task {
        id: entity.id.to_string(),
        project: read_str(&entity.data, "project"),
        description: read_str(&entity.data, "description"),
        state: read_str(&entity.data, "state")
            .parse()
            .unwrap_or(TaskState::Proposed),
        claimed_by: entity
            .data
            .get("claimed_by")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
        blocked_by: read_json_array(&entity.data, "blocked_by"),
        checkpoint: entity
            .data
            .get("checkpoint")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
        created_at: read_str(&entity.data, "created_at"),
        updated_at: read_str(&entity.data, "updated_at"),
    })
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
    fn propose_and_claim() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = TaskStore::new();
        let task = store
            .propose(graph, "magellan".into(), "fix".into(), vec![])
            .unwrap();
        assert_eq!(task.state, TaskState::Proposed);
        let claimed = store.claim(graph, &task.id, "agent-1".into()).unwrap();
        assert_eq!(claimed.state, TaskState::Claimed);
        assert_eq!(claimed.claimed_by, Some("agent-1".into()));
    }

    #[test]
    fn reject_double_claim() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = TaskStore::new();
        let task = store
            .propose(graph, "m".into(), "fix".into(), vec![])
            .unwrap();
        store.claim(graph, &task.id, "a".into()).unwrap();
        assert!(store.claim(graph, &task.id, "b".into()).is_err());
    }

    #[test]
    fn state_transitions() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = TaskStore::new();
        let task = store
            .propose(graph, "m".into(), "fix".into(), vec![])
            .unwrap();
        store.claim(graph, &task.id, "a".into()).unwrap();
        let updated = store
            .update_state(
                graph,
                &task.id,
                TaskState::InProgress,
                Some("impl".into()),
                Some("a"),
            )
            .unwrap();
        assert_eq!(updated.state, TaskState::InProgress);
    }

    #[test]
    fn reject_invalid_transition() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = TaskStore::new();
        let task = store
            .propose(graph, "m".into(), "fix".into(), vec![])
            .unwrap();
        assert!(store
            .update_state(graph, &task.id, TaskState::Done, None, None)
            .is_err());
    }

    #[test]
    fn find_blocked_by() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = TaskStore::new();
        let a = store
            .propose(graph, "m".into(), "A".into(), vec![])
            .unwrap();
        store
            .propose(graph, "m".into(), "B".into(), vec![a.id.clone()])
            .unwrap();
        assert_eq!(store.find_blocked_by(graph, &a.id).unwrap().len(), 1);
    }

    #[test]
    fn claim_next_oldest_proposed() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = TaskStore::new();
        let a = store
            .propose(graph, "m".into(), "A".into(), vec![])
            .unwrap();
        let b = store
            .propose(graph, "m".into(), "B".into(), vec![])
            .unwrap();
        let next = store.claim_next(graph, "m", "agent-1".into()).unwrap();
        assert_eq!(next.id, a.id); // oldest
        assert_eq!(next.state, TaskState::Claimed);
        assert_eq!(next.claimed_by, Some("agent-1".into()));
        // b still proposed
        assert_eq!(store.get(graph, &b.id).unwrap().state, TaskState::Proposed);
    }

    #[test]
    fn claim_next_empty_project() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = TaskStore::new();
        assert!(store.claim_next(graph, "no-project", "a".into()).is_err());
    }

    #[test]
    fn reclaim_stale() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let store = TaskStore::new();
        let task = store
            .propose(graph, "m".into(), "fix".into(), vec![])
            .unwrap();
        store.claim(graph, &task.id, "agent-1".into()).unwrap();
        let reclaimed = store.reclaim_stale(graph, "agent-1").unwrap();
        assert_eq!(reclaimed.len(), 1);
        assert_eq!(
            store.get(graph, &task.id).unwrap().state,
            TaskState::Proposed
        );
    }
}
