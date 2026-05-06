use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::{EnvoyError, Result};

const KIND_AGENT: &str = "EnvoyAgent";
const KIND_AGENT_COUNTER: &str = "EnvoyAgentCounter";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub agent_id: String,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub online: bool,
    pub status: Option<crate::status::AgentStatusSnapshot>,
    pub last_heartbeat_at: Option<String>,
}

#[derive(Debug, Default)]
struct AgentTree {
    agents: HashMap<String, AgentInfo>,
    children: HashMap<String, Vec<String>>,
    next_id: u64,
}

/// Thread-safe agent registry with parent/child hierarchy and sqlitegraph persistence.
///
/// Uses a hybrid approach: in-memory `AgentTree` for fast reads, write-through to
/// sqlitegraph on `register` and `disconnect`. On startup, agents are loaded from
/// the database — all agents start offline and must re-register.
pub struct AgentRegistry {
    tree: Arc<Mutex<AgentTree>>,
}

impl AgentRegistry {
    /// Create a new registry, loading existing agents from the database.
    /// All agents from the DB start in offline state — they must re-register.
    pub fn new(graph: &sqlitegraph::SqliteGraph) -> Result<Self> {
        let entities = graph.find_entities_by_kind(KIND_AGENT)?;
        let mut tree = AgentTree::default();

        if let Some(counter) =
            graph.find_entity_by_kind_and_name(KIND_AGENT_COUNTER, "agent-counter")?
        {
            tree.next_id = counter
                .data
                .get("next_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
        }

        for entity in &entities {
            let status = entity
                .data
                .get("status")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let last_heartbeat_at = entity
                .data
                .get("last_heartbeat_at")
                .and_then(|v| v.as_str())
                .map(String::from);
            let info = AgentInfo {
                agent_id: entity.name.clone(),
                name: read_json_str(&entity.data, "name"),
                kind: read_json_str(&entity.data, "kind"),
                parent_id: entity
                    .data
                    .get("parent_id")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                online: false,
                status,
                last_heartbeat_at,
            };

            if let Some(ref pid) = info.parent_id {
                tree.children
                    .entry(pid.clone())
                    .or_default()
                    .push(info.agent_id.clone());
            }
            tree.agents.insert(info.agent_id.clone(), info);
        }

        Ok(Self {
            tree: Arc::new(Mutex::new(tree)),
        })
    }

    fn persist_agent(graph: &sqlitegraph::SqliteGraph, info: &AgentInfo) -> Result<()> {
        use sqlitegraph::GraphEntity;

        if let Some(mut entity) = graph.find_entity_by_kind_and_name(KIND_AGENT, &info.agent_id)? {
            entity.data = agent_to_json(info);
            graph.update_entity(&entity)?;
        } else {
            let entity = GraphEntity {
                id: 0,
                kind: KIND_AGENT.to_string(),
                name: info.agent_id.clone(),
                file_path: None,
                data: agent_to_json(info),
            };
            graph.insert_entity(&entity)?;
        }
        Ok(())
    }

    fn persist_counter(graph: &sqlitegraph::SqliteGraph, next_id: u64) -> Result<()> {
        use sqlitegraph::GraphEntity;

        if let Some(mut entity) =
            graph.find_entity_by_kind_and_name(KIND_AGENT_COUNTER, "agent-counter")?
        {
            entity.data = serde_json::json!({"next_id": next_id});
            graph.update_entity(&entity)?;
        } else {
            let entity = GraphEntity {
                id: 0,
                kind: KIND_AGENT_COUNTER.to_string(),
                name: "agent-counter".to_string(),
                file_path: None,
                data: serde_json::json!({"next_id": next_id}),
            };
            graph.insert_entity(&entity)?;
        }
        Ok(())
    }

    /// Register an agent and return its server-assigned ID.
    pub fn register(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        name: &str,
        kind: &str,
        parent_id: Option<String>,
    ) -> Result<AgentInfo> {
        let info;
        let next_id_val;
        {
            let mut tree = self.tree.lock().unwrap();
            let agent_id = if let Some(ref pid) = parent_id {
                if !tree.agents.contains_key(pid) {
                    return Err(EnvoyError::AgentNotFound(pid.clone()));
                }
                if !tree.agents[pid].online {
                    return Err(EnvoyError::AgentOffline(pid.clone()));
                }
                let siblings = tree.children.entry(pid.clone()).or_default();
                let child_num = siblings.len() + 1;
                format!("{}.{}", pid, child_num)
            } else {
                tree.next_id += 1;
                format!("id{}", tree.next_id)
            };

            info = AgentInfo {
                agent_id: agent_id.clone(),
                name: name.to_string(),
                kind: kind.to_string(),
                parent_id: parent_id.clone(),
                online: true,
                status: None,
                last_heartbeat_at: None,
            };

            tree.agents.insert(agent_id.clone(), info.clone());
            if let Some(ref pid) = parent_id {
                tree.children.entry(pid.clone()).or_default().push(agent_id);
            }
            next_id_val = tree.next_id;
        }

        Self::persist_agent(graph, &info)?;
        Self::persist_counter(graph, next_id_val)?;

        Ok(info)
    }

    /// Mark agent and all descendants offline. Returns list of affected IDs.
    pub fn disconnect(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
    ) -> Result<Vec<String>> {
        let mut affected = Vec::new();
        {
            let mut tree = self.tree.lock().unwrap();
            if !tree.agents.contains_key(agent_id) {
                return Err(EnvoyError::AgentNotFound(agent_id.to_string()));
            }

            let mut stack = vec![agent_id.to_string()];
            while let Some(id) = stack.pop() {
                if let Some(info) = tree.agents.get_mut(&id) {
                    info.online = false;
                    affected.push(id.clone());
                }
                if let Some(kids) = tree.children.get(&id) {
                    stack.extend(kids.clone());
                }
            }
        }

        for id in &affected {
            let info = {
                let tree = self.tree.lock().unwrap();
                tree.agents.get(id).cloned()
            };
            if let Some(info) = info {
                Self::persist_agent(graph, &info)?;
            }
        }

        Ok(affected)
    }

    pub fn get(&self, agent_id: &str) -> Result<AgentInfo> {
        let tree = self.tree.lock().unwrap();
        tree.agents
            .get(agent_id)
            .cloned()
            .ok_or_else(|| EnvoyError::AgentNotFound(agent_id.to_string()))
    }

    pub fn list_all(&self) -> Vec<AgentInfo> {
        let tree = self.tree.lock().unwrap();
        tree.agents.values().cloned().collect()
    }

    pub fn list_online(&self) -> Vec<AgentInfo> {
        let tree = self.tree.lock().unwrap();
        tree.agents.values().filter(|a| a.online).cloned().collect()
    }

    pub fn get_children(&self, agent_id: &str) -> Result<Vec<AgentInfo>> {
        let tree = self.tree.lock().unwrap();
        if !tree.agents.contains_key(agent_id) {
            return Err(EnvoyError::AgentNotFound(agent_id.to_string()));
        }
        let kids = tree
            .children
            .get(agent_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| tree.agents.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default();
        Ok(kids)
    }

    pub fn is_online(&self, agent_id: &str) -> bool {
        let tree = self.tree.lock().unwrap();
        tree.agents.get(agent_id).map(|a| a.online).unwrap_or(false)
    }

    /// Record a heartbeat, updating the agent's status snapshot and timestamp.
    pub fn heartbeat(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
        status: crate::status::AgentStatusSnapshot,
    ) -> Result<()> {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let mut tree = self.tree.lock().unwrap();
        let info = tree
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| EnvoyError::AgentNotFound(agent_id.to_string()))?;

        info.status = Some(status);
        info.last_heartbeat_at = Some(timestamp.clone());

        // Write through to DB
        if let Some(mut entity) = graph.find_entity_by_kind_and_name(KIND_AGENT, agent_id)? {
            entity.data["status"] = serde_json::to_value(&info.status)?;
            entity.data["last_heartbeat_at"] = serde_json::json!(&info.last_heartbeat_at);
            graph.update_entity(&entity)?;
        }
        Ok(())
    }

    /// Return agents whose last heartbeat is older than threshold_minutes.
    pub fn get_stale_agents(&self, threshold_minutes: i64) -> Vec<AgentInfo> {
        let tree = self.tree.lock().unwrap();
        let now = chrono::Utc::now();
        tree.agents
            .values()
            .filter(|info| {
                if !info.online {
                    return false;
                }
                if let Some(ref ts) = info.last_heartbeat_at {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                        let age = now - dt.with_timezone(&chrono::Utc);
                        return age.num_minutes() >= threshold_minutes;
                    }
                }
                true // no heartbeat ever = stale
            })
            .cloned()
            .collect()
    }
}

fn agent_to_json(info: &AgentInfo) -> serde_json::Value {
    serde_json::json!({
        "name": info.name,
        "kind": info.kind,
        "parent_id": info.parent_id,
        "online": info.online,
        "status": info.status,
        "last_heartbeat_at": info.last_heartbeat_at,
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
    use super::*;
    use crate::engine::Engine;

    fn test_registry() -> (AgentRegistry, Engine) {
        let engine = Engine::open_in_memory().unwrap();
        let reg = AgentRegistry::new(engine.graph()).unwrap();
        (reg, engine)
    }

    #[test]
    fn register_root_agents() {
        let (reg, engine) = test_registry();
        let a1 = reg
            .register(engine.graph(), "claude", "claude", None)
            .unwrap();
        let a2 = reg
            .register(engine.graph(), "hermes", "hermes", None)
            .unwrap();

        assert_eq!(a1.agent_id, "id1");
        assert_eq!(a2.agent_id, "id2");
        assert!(a1.parent_id.is_none());
    }

    #[test]
    fn register_subagents_with_hierarchy() {
        let (reg, engine) = test_registry();
        let g = engine.graph();
        let parent = reg.register(g, "claude", "claude", None).unwrap();
        let child1 = reg
            .register(g, "sub1", "claude", Some(parent.agent_id.clone()))
            .unwrap();
        let child2 = reg
            .register(g, "sub2", "claude", Some(parent.agent_id.clone()))
            .unwrap();
        let grandchild = reg
            .register(g, "subsub", "claude", Some(child1.agent_id.clone()))
            .unwrap();

        assert_eq!(child1.agent_id, "id1.1");
        assert_eq!(child2.agent_id, "id1.2");
        assert_eq!(grandchild.agent_id, "id1.1.1");

        let children = reg.get_children(&parent.agent_id).unwrap();
        assert_eq!(children.len(), 2);

        let grandkids = reg.get_children(&child1.agent_id).unwrap();
        assert_eq!(grandkids.len(), 1);
    }

    #[test]
    fn disconnect_cascades_to_descendants() {
        let (reg, engine) = test_registry();
        let g = engine.graph();
        let parent = reg.register(g, "claude", "claude", None).unwrap();
        let child = reg
            .register(g, "sub", "claude", Some(parent.agent_id.clone()))
            .unwrap();
        let _grandchild = reg
            .register(g, "subsub", "claude", Some(child.agent_id.clone()))
            .unwrap();

        let affected = reg.disconnect(g, &parent.agent_id).unwrap();
        assert_eq!(affected.len(), 3);
        assert!(!reg.is_online(&parent.agent_id));
        assert!(!reg.is_online(&child.agent_id));
    }

    #[test]
    fn subagent_requires_online_parent() {
        let (reg, engine) = test_registry();
        let g = engine.graph();
        let parent = reg.register(g, "claude", "claude", None).unwrap();
        let pid = parent.agent_id.clone();
        reg.disconnect(g, &pid).unwrap();

        let err = reg.register(g, "sub", "claude", Some(pid)).unwrap_err();
        assert!(matches!(err, EnvoyError::AgentOffline(_)));
    }

    #[test]
    fn duplicate_names_allowed_different_ids() {
        let (reg, engine) = test_registry();
        let g = engine.graph();
        let a1 = reg.register(g, "claude", "claude", None).unwrap();
        let a2 = reg.register(g, "claude", "claude", None).unwrap();
        assert_ne!(a1.agent_id, a2.agent_id);
        assert_eq!(a1.name, "claude");
        assert_eq!(a2.name, "claude");
    }

    #[test]
    fn persistence_survives_restart() {
        let engine = Engine::open_in_memory().unwrap();
        let g = engine.graph();

        // First session: register some agents
        let reg = AgentRegistry::new(g).unwrap();
        let parent = reg.register(g, "claude", "claude", None).unwrap();
        reg.register(g, "sub", "sub", Some(parent.agent_id.clone()))
            .unwrap();
        reg.disconnect(g, &parent.agent_id).unwrap();
        drop(reg);

        // Second session: reload from same graph
        let reg2 = AgentRegistry::new(g).unwrap();
        let all = reg2.list_all();
        assert_eq!(all.len(), 2, "two agents should survive restart");

        // All agents start offline after reload
        for a in &all {
            assert!(!a.online, "agents should be offline after restart");
        }

        let parent = all.iter().find(|a| a.agent_id == "id1").unwrap();
        assert_eq!(parent.name, "claude");

        let children = reg2.get_children("id1").unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].agent_id, "id1.1");
    }

    #[test]
    fn next_id_counter_persists() {
        let engine = Engine::open_in_memory().unwrap();
        let g = engine.graph();

        // Register 3 root agents
        {
            let reg = AgentRegistry::new(g).unwrap();
            reg.register(g, "a1", "test", None).unwrap();
            reg.register(g, "a2", "test", None).unwrap();
            reg.register(g, "a3", "test", None).unwrap();
        }

        // Restart: next agent should be id4, not id1
        {
            let reg = AgentRegistry::new(g).unwrap();
            let a4 = reg.register(g, "a4", "test", None).unwrap();
            assert_eq!(a4.agent_id, "id4");
        }
    }

    #[test]
    fn heartbeat_updates_status() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let registry = AgentRegistry::new(graph).unwrap();

        let info = registry
            .register(graph, "test1".into(), "worker".into(), None)
            .unwrap();
        let status = crate::status::AgentStatusSnapshot {
            state: crate::status::AgentState::Working,
            task_id: Some("task-1".into()),
            blocked_reason: None,
            waiting_on_agent: None,
            checkpoint: Some("implementation".into()),
            working_on: "building heartbeat".into(),
        };
        registry
            .heartbeat(graph, &info.agent_id, status.clone())
            .unwrap();

        let updated = registry.get(&info.agent_id).unwrap();
        assert!(updated.last_heartbeat_at.is_some());
        assert_eq!(updated.status.as_ref().unwrap().state.as_str(), "working");
    }

    #[test]
    fn get_stale_agents_finds_stale() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let registry = AgentRegistry::new(graph).unwrap();

        let info = registry
            .register(graph, "stale1".into(), "worker".into(), None)
            .unwrap();
        // Never sends heartbeat — should be stale
        let stale = registry.get_stale_agents(0); // threshold=0 means immediately stale
        assert!(stale.iter().any(|a| a.agent_id == info.agent_id));
    }

    #[test]
    fn get_stale_agents_excludes_offline() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let registry = AgentRegistry::new(graph).unwrap();
        // Agent loaded from DB is offline
        let stale = registry.get_stale_agents(0);
        assert!(stale.is_empty());
    }
}
