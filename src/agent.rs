use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::{EnvoyError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub agent_id: String,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub online: bool,
}

#[derive(Debug, Default)]
struct AgentTree {
    /// All agents by ID.
    agents: HashMap<String, AgentInfo>,
    /// Children of each agent ID.
    children: HashMap<String, Vec<String>>,
    /// Counter for generating root agent IDs.
    next_id: u64,
}

/// Thread-safe agent registry with parent/child hierarchy.
pub struct AgentRegistry {
    tree: Arc<Mutex<AgentTree>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            tree: Arc::new(Mutex::new(AgentTree::default())),
        }
    }

    /// Register an agent and return its server-assigned ID.
    pub fn register(&self, name: &str, kind: &str, parent_id: Option<String>) -> Result<AgentInfo> {
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

        let info = AgentInfo {
            agent_id: agent_id.clone(),
            name: name.to_string(),
            kind: kind.to_string(),
            parent_id: parent_id.clone(),
            online: true,
        };

        tree.agents.insert(agent_id.clone(), info.clone());
        if let Some(ref pid) = parent_id {
            tree.children
                .entry(pid.clone())
                .or_default()
                .push(agent_id);
        }

        Ok(info)
    }

    /// Mark agent and all descendants offline. Returns list of affected IDs.
    pub fn disconnect(&self, agent_id: &str) -> Result<Vec<String>> {
        let mut tree = self.tree.lock().unwrap();
        if !tree.agents.contains_key(agent_id) {
            return Err(EnvoyError::AgentNotFound(agent_id.to_string()));
        }

        let mut affected = Vec::new();
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
        tree.agents
            .get(agent_id)
            .map(|a| a.online)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_root_agents() {
        let reg = AgentRegistry::new();
        let a1 = reg.register("claude", "claude", None).unwrap();
        let a2 = reg.register("hermes", "hermes", None).unwrap();

        assert_eq!(a1.agent_id, "id1");
        assert_eq!(a2.agent_id, "id2");
        assert!(a1.parent_id.is_none());
    }

    #[test]
    fn register_subagents_with_hierarchy() {
        let reg = AgentRegistry::new();
        let parent = reg.register("claude", "claude", None).unwrap();
        let child1 = reg
            .register("sub1", "claude", Some(parent.agent_id.clone()))
            .unwrap();
        let child2 = reg
            .register("sub2", "claude", Some(parent.agent_id.clone()))
            .unwrap();
        let grandchild = reg
            .register("subsub", "claude", Some(child1.agent_id.clone()))
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
        let reg = AgentRegistry::new();
        let parent = reg.register("claude", "claude", None).unwrap();
        let child = reg
            .register("sub", "claude", Some(parent.agent_id.clone()))
            .unwrap();
        let _grandchild = reg
            .register("subsub", "claude", Some(child.agent_id.clone()))
            .unwrap();

        let affected = reg.disconnect(&parent.agent_id).unwrap();
        assert_eq!(affected.len(), 3);
        assert!(!reg.is_online(&parent.agent_id));
        assert!(!reg.is_online(&child.agent_id));
    }

    #[test]
    fn subagent_requires_online_parent() {
        let reg = AgentRegistry::new();
        let parent = reg.register("claude", "claude", None).unwrap();
        let pid = parent.agent_id.clone();
        reg.disconnect(&pid).unwrap();

        let err = reg.register("sub", "claude", Some(pid)).unwrap_err();
        assert!(matches!(err, EnvoyError::AgentOffline(_)));
    }

    #[test]
    fn duplicate_names_allowed_different_ids() {
        let reg = AgentRegistry::new();
        let a1 = reg.register("claude", "claude", None).unwrap();
        let a2 = reg.register("claude", "claude", None).unwrap();
        assert_ne!(a1.agent_id, a2.agent_id);
        assert_eq!(a1.name, "claude");
        assert_eq!(a2.name, "claude");
    }
}
