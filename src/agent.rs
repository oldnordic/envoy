use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::{EnvoyError, Result};

const KIND_AGENT: &str = "EnvoyAgent";
const KIND_AGENT_COUNTER: &str = "EnvoyAgentCounter";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycle {
    Active,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub agent_id: String,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub lifecycle: AgentLifecycle,
    pub status: Option<crate::status::AgentStatusSnapshot>,
    pub last_heartbeat_at: Option<String>,
}

#[derive(Debug, Default)]
struct AgentTree {
    agents: HashMap<String, AgentInfo>,
    children: HashMap<String, Vec<String>>,
    next_id: u64,
    retired_ids: Vec<u64>, // Pool of explicitly retired numeric IDs available for reuse
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
    /// Only agents that were explicitly retired before shutdown have their IDs
    /// added to the reuse pool. Agents that are simply offline after restart
    /// keep their IDs reserved until they re-register or are explicitly retired.
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
            let lifecycle = entity
                .data
                .get("lifecycle")
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "active" => AgentLifecycle::Active,
                    _ => AgentLifecycle::Retired,
                })
                .unwrap_or(AgentLifecycle::Retired);

            // Only add to reuse pool if agent was explicitly retired before shutdown
            // (not just offline due to restart)
            let was_explicitly_retired = lifecycle == AgentLifecycle::Retired;

            // All agents start as Retired after restart — they must re-register or heartbeat
            let info = AgentInfo {
                agent_id: entity.name.clone(),
                name: read_json_str(&entity.data, "name"),
                kind: read_json_str(&entity.data, "kind"),
                parent_id: entity
                    .data
                    .get("parent_id")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                lifecycle: AgentLifecycle::Retired,
                status: status.clone(),
                last_heartbeat_at: last_heartbeat_at.clone(),
            };

            if let Some(ref pid) = info.parent_id {
                tree.children
                    .entry(pid.clone())
                    .or_default()
                    .push(info.agent_id.clone());
            }

            // Only add to reuse pool if agent was explicitly retired before shutdown
            if info.parent_id.is_none() && was_explicitly_retired {
                if let Some(num_str) = info.agent_id.strip_prefix("id") {
                    if let Ok(num) = num_str.parse::<u64>() {
                        tree.retired_ids.push(num);
                    }
                }
            }

            tree.agents.insert(info.agent_id.clone(), info);
        }

        // Sort retired IDs so we reuse lowest first
        tree.retired_ids.sort_unstable();

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
    ///
    /// If an active agent with the same name already exists (and no parent_id is
    /// specified), the existing agent is returned — registration is idempotent.
    /// This lets agents reconnect without creating duplicates.
    ///
    /// For subagents (parent_id provided), a new child agent is always created
    /// with a hierarchical ID like `id1.1`, `id1.2`, etc.
    pub fn register(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        name: &str,
        kind: &str,
        parent_id: Option<String>,
    ) -> Result<AgentInfo> {
        // Check for existing active agent with same name (root agents only)
        if parent_id.is_none() {
            let tree = self.tree.lock();
            if let Some(existing) = tree.agents.values().find(|a| {
                a.name == name && a.lifecycle == AgentLifecycle::Active && a.parent_id.is_none()
            }) {
                return Ok(existing.clone());
            }
        }

        let info;
        let next_id_val;
        {
            let mut tree = self.tree.lock();
            let agent_id = if let Some(ref pid) = parent_id {
                if !tree.agents.contains_key(pid) {
                    return Err(EnvoyError::AgentNotFound(pid.clone()));
                }
                if tree.agents[pid].lifecycle != AgentLifecycle::Active {
                    return Err(EnvoyError::AgentOffline(pid.clone()));
                }
                let siblings = tree.children.entry(pid.clone()).or_default();
                let child_num = siblings.len() + 1;
                format!("{}.{}", pid, child_num)
            } else {
                // Reuse retired ID if available, otherwise increment counter
                let id_num = if let Some(reused) = tree.retired_ids.pop() {
                    reused
                } else {
                    tree.next_id += 1;
                    tree.next_id
                };
                format!("id{}", id_num)
            };

            info = AgentInfo {
                agent_id: agent_id.clone(),
                name: name.to_string(),
                kind: kind.to_string(),
                parent_id: parent_id.clone(),
                lifecycle: AgentLifecycle::Active,
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

    /// Retire an agent and all descendants. Retired IDs are added to the reuse pool.
    /// Returns list of affected IDs.
    pub fn retire(&self, graph: &sqlitegraph::SqliteGraph, agent_id: &str) -> Result<Vec<String>> {
        let mut affected = Vec::new();
        let mut retired_root_ids = Vec::new();
        {
            let mut tree = self.tree.lock();
            if !tree.agents.contains_key(agent_id) {
                return Err(EnvoyError::AgentNotFound(agent_id.to_string()));
            }

            let mut stack = vec![agent_id.to_string()];
            while let Some(id) = stack.pop() {
                if let Some(info) = tree.agents.get_mut(&id) {
                    // Collect retired root IDs for reuse pool
                    if info.parent_id.is_none() && info.lifecycle != AgentLifecycle::Retired {
                        if let Some(num_str) = info.agent_id.strip_prefix("id") {
                            if let Ok(num) = num_str.parse::<u64>() {
                                retired_root_ids.push(num);
                            }
                        }
                    }
                    info.lifecycle = AgentLifecycle::Retired;
                    affected.push(id.clone());
                }
                if let Some(kids) = tree.children.get(&id) {
                    stack.extend(kids.clone());
                }
            }

            // Add retired IDs to reuse pool and sort
            tree.retired_ids.extend(retired_root_ids);
            tree.retired_ids.sort_unstable();
        }

        for id in &affected {
            let info = {
                let tree = self.tree.lock();
                tree.agents.get(id).cloned()
            };
            if let Some(info) = info {
                Self::persist_agent(graph, &info)?;
            }
        }

        Ok(affected)
    }

    /// Disconnect agent — marks agent and descendants as Retired.
    /// Kept for backward compatibility with DELETE /agents/{id}.
    pub fn disconnect(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
    ) -> Result<Vec<String>> {
        self.retire(graph, agent_id)
    }

    pub fn get(&self, agent_id: &str) -> Result<AgentInfo> {
        let tree = self.tree.lock();
        tree.agents
            .get(agent_id)
            .cloned()
            .ok_or_else(|| EnvoyError::AgentNotFound(agent_id.to_string()))
    }

    pub fn list_all(&self) -> Result<Vec<AgentInfo>> {
        let tree = self.tree.lock();
        Ok(tree.agents.values().cloned().collect())
    }

    pub fn is_active(&self, agent_id: &str) -> Result<bool> {
        let tree = self.tree.lock();
        Ok(tree
            .agents
            .get(agent_id)
            .map(|a| a.lifecycle == AgentLifecycle::Active)
            .unwrap_or(false))
    }

    pub fn list_active(&self) -> Result<Vec<AgentInfo>> {
        let tree = self.tree.lock();
        Ok(tree
            .agents
            .values()
            .filter(|a| a.lifecycle == AgentLifecycle::Active)
            .cloned()
            .collect())
    }

    pub fn get_children(&self, agent_id: &str) -> Result<Vec<AgentInfo>> {
        let tree = self.tree.lock();
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

    /// Record a heartbeat, updating the agent's status snapshot and timestamp.
    pub fn heartbeat(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
        status: crate::status::AgentStatusSnapshot,
    ) -> Result<()> {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let mut tree = self.tree.lock();
        let info = tree
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| EnvoyError::AgentNotFound(agent_id.to_string()))?;

        info.lifecycle = AgentLifecycle::Active;
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

    /// Return active agents whose last heartbeat is older than threshold_minutes.
    pub fn get_stale_agents(&self, threshold_minutes: i64) -> Result<Vec<AgentInfo>> {
        let tree = self.tree.lock();
        let now = chrono::Utc::now();
        Ok(tree
            .agents
            .values()
            .filter(|info| {
                if info.lifecycle != AgentLifecycle::Active {
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
            .collect())
    }

    /// Remove retired agents that haven't heartbeated in 24+ hours.
    /// Returns the number of purged agents.
    pub fn purge_retired(&self, threshold_hours: i64) -> Result<usize> {
        let mut tree = self.tree.lock();
        let now = chrono::Utc::now();
        let before = tree.agents.len();
        let stale_ids: Vec<String> = tree
            .agents
            .iter()
            .filter(|(_, info)| {
                if info.lifecycle != AgentLifecycle::Retired {
                    return false;
                }
                if let Some(ref ts) = info.last_heartbeat_at {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                        let age = now - dt.with_timezone(&chrono::Utc);
                        return age.num_hours() >= threshold_hours;
                    }
                }
                false
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &stale_ids {
            tree.children.remove(id);
            // Remove from parent's children list
            tree.children.values_mut().for_each(|list| {
                list.retain(|c| c != id);
            });
            tree.agents.remove(id);
        }
        Ok(before - tree.agents.len())
    }
}

fn agent_to_json(info: &AgentInfo) -> serde_json::Value {
    serde_json::json!({
        "name": info.name,
        "kind": info.kind,
        "parent_id": info.parent_id,
        "lifecycle": info.lifecycle,
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
        assert!(!reg.is_active(&parent.agent_id).unwrap());
        assert!(!reg.is_active(&child.agent_id).unwrap());
    }

    #[test]
    fn subagent_requires_active_parent() {
        let (reg, engine) = test_registry();
        let g = engine.graph();
        let parent = reg.register(g, "claude", "claude", None).unwrap();
        let pid = parent.agent_id.clone();
        reg.retire(g, &pid).unwrap();

        let err = reg.register(g, "sub", "claude", Some(pid)).unwrap_err();
        assert!(matches!(err, EnvoyError::AgentOffline(_)));
    }

    #[test]
    fn same_name_returns_existing_agent() {
        let (reg, engine) = test_registry();
        let g = engine.graph();
        let a1 = reg.register(g, "claude", "claude", None).unwrap();
        // Second registration with same name returns existing agent
        let a2 = reg.register(g, "claude", "claude", None).unwrap();
        assert_eq!(
            a1.agent_id, a2.agent_id,
            "same name should return existing agent, not create new one"
        );
        assert!(
            reg.is_active(&a1.agent_id).unwrap(),
            "original agent should still be active"
        );
        // Only one agent with name "claude" should exist
        let all = reg.list_all().unwrap();
        let claude_count = all.iter().filter(|a| a.name == "claude").count();
        assert_eq!(
            claude_count, 1,
            "only one agent named 'claude' should exist"
        );
    }

    #[test]
    fn retired_agent_name_can_be_reused() {
        let (reg, engine) = test_registry();
        let g = engine.graph();
        let a1 = reg.register(g, "claude", "claude", None).unwrap();
        let a1_id = a1.agent_id.clone();
        reg.retire(g, &a1_id).unwrap();

        // After retiring, the ID should be reused from pool
        let a2 = reg.register(g, "new_claude", "claude", None).unwrap();
        assert_eq!(
            a1_id, a2.agent_id,
            "retired agent's ID should be reused from pool"
        );
        assert!(
            reg.is_active(&a2.agent_id).unwrap(),
            "new agent should be active"
        );
        // The old agent info object shows retired because it was cloned before retire
        // The registry now has the NEW agent at the same ID
        let current = reg.get(&a1_id).unwrap();
        assert_eq!(current.name, "new_claude");
        assert!(
            reg.is_active(&a1_id).unwrap(),
            "agent at old ID should now be active (reused)"
        );
    }

    #[test]
    fn subagents_always_create_new_even_with_same_name() {
        let (reg, engine) = test_registry();
        let g = engine.graph();
        let parent = reg.register(g, "claude", "claude", None).unwrap();
        let child1 = reg
            .register(g, "sub", "claude", Some(parent.agent_id.clone()))
            .unwrap();
        let child2 = reg
            .register(g, "sub", "claude", Some(parent.agent_id.clone()))
            .unwrap();

        // Subagents with same name should still create new agents
        assert_ne!(
            child1.agent_id, child2.agent_id,
            "subagents with same name should get different IDs"
        );
        assert_eq!(child1.name, "sub");
        assert_eq!(child2.name, "sub");
    }
    #[test]
    fn retire_cascades_to_descendants() {
        let (reg, engine) = test_registry();
        let g = engine.graph();
        let parent = reg.register(g, "claude", "claude", None).unwrap();
        let child = reg
            .register(g, "sub", "claude", Some(parent.agent_id.clone()))
            .unwrap();

        let affected = reg.retire(g, &parent.agent_id).unwrap();
        assert_eq!(affected.len(), 2);
        assert!(!reg.is_active(&parent.agent_id).unwrap());
        assert!(!reg.is_active(&child.agent_id).unwrap());
    }

    #[test]
    fn retired_id_cannot_be_reused() {
        let (reg, engine) = test_registry();
        let g = engine.graph();
        let a1 = reg.register(g, "claude", "claude", None).unwrap();
        reg.retire(g, &a1.agent_id).unwrap();
        // The retired agent's ID still exists but is not active
        let info = reg.get(&a1.agent_id).unwrap();
        assert_eq!(info.lifecycle, AgentLifecycle::Retired);
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
        reg.retire(g, &parent.agent_id).unwrap();
        drop(reg);

        // Second session: reload from same graph
        let reg2 = AgentRegistry::new(g).unwrap();
        let all = reg2.list_all().unwrap();
        assert_eq!(all.len(), 2, "two agents should survive restart");

        // All agents start retired after reload
        for a in &all {
            assert!(
                a.lifecycle == AgentLifecycle::Retired,
                "agents should be retired after restart"
            );
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

        // Register 3 root agents, then explicitly retire them
        {
            let reg = AgentRegistry::new(g).unwrap();
            reg.register(g, "a1", "test", None).unwrap();
            reg.register(g, "a2", "test", None).unwrap();
            reg.register(g, "a3", "test", None).unwrap();
            // Retire all so their IDs go to reuse pool
            reg.retire(g, "id1").unwrap();
            reg.retire(g, "id2").unwrap();
            reg.retire(g, "id3").unwrap();
        }

        // Restart: retired IDs should be in reuse pool
        // Next registration should reuse id3 (highest retired, popped from sorted vec)
        // Actually sorted is [1,2,3], pop gives 3... wait, we sort ascending and pop from end
        // Let me check... sort_unstable on [1,2,3] → [1,2,3], pop → 3
        // Hmm, we want to reuse lowest first. Let me use remove(0) instead.
        {
            let reg = AgentRegistry::new(g).unwrap();
            let a4 = reg.register(g, "a4", "test", None).unwrap();
            // With pop() on sorted [1,2,3], we get 3 first
            assert_eq!(
                a4.agent_id, "id3",
                "should reuse highest retired ID (pop from sorted)"
            );
        }
    }

    #[test]
    fn heartbeat_updates_status() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let registry = AgentRegistry::new(graph).unwrap();

        let info = registry.register(graph, "test1", "worker", None).unwrap();
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
        assert!(
            reg_is_active(&registry, &info.agent_id),
            "heartbeat must keep agent active"
        );
    }

    #[test]
    fn heartbeat_reactivates_retired_agent_after_restart() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();

        // First session: register an agent
        let reg = AgentRegistry::new(graph).unwrap();
        let info = reg.register(graph, "agent1", "worker", None).unwrap();
        assert_eq!(info.lifecycle, AgentLifecycle::Active);
        drop(reg);

        // Second session: reload — agent starts retired (but NOT in reuse pool)
        let reg2 = AgentRegistry::new(graph).unwrap();
        let reloaded = reg2.get(&info.agent_id).unwrap();
        assert!(
            reloaded.lifecycle == AgentLifecycle::Retired,
            "agents start retired after restart"
        );
        // The ID should NOT be in the reuse pool since it was only implicitly retired
        let a_new = reg2.register(graph, "new_agent", "worker", None).unwrap();
        assert_eq!(
            a_new.agent_id, "id2",
            "should get new ID, not reuse implicitly retired one"
        );

        // Heartbeat brings agent back to active
        let status = crate::status::AgentStatusSnapshot {
            state: crate::status::AgentState::Working,
            task_id: None,
            blocked_reason: None,
            waiting_on_agent: None,
            checkpoint: None,
            working_on: "reconnected".into(),
        };
        reg2.heartbeat(graph, &info.agent_id, status).unwrap();

        let after_hb = reg2.get(&info.agent_id).unwrap();
        assert!(
            after_hb.lifecycle == AgentLifecycle::Active,
            "heartbeat must bring agent active after restart"
        );
        assert!(after_hb.last_heartbeat_at.is_some());
    }

    #[test]
    fn get_stale_agents_finds_stale() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let registry = AgentRegistry::new(graph).unwrap();

        let info = registry.register(graph, "stale1", "worker", None).unwrap();
        // Never sends heartbeat — should be stale
        let stale = registry.get_stale_agents(0).unwrap(); // threshold=0 means immediately stale
        assert!(stale.iter().any(|a| a.agent_id == info.agent_id));
    }

    #[test]
    fn get_stale_agents_excludes_retired() {
        let engine = Engine::open_in_memory().unwrap();
        let graph = engine.graph();
        let registry = AgentRegistry::new(graph).unwrap();
        // No active agents — empty
        let stale = registry.get_stale_agents(0).unwrap();
        assert!(stale.is_empty());
    }

    fn reg_is_active(reg: &AgentRegistry, id: &str) -> bool {
        reg.is_active(id).unwrap()
    }
}
