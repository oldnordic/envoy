//! Rate limit persistence via sqlitegraph.
//!
//! Stores per-agent rate limit state in the database for:
//! - Recovery across restarts
//! - Cross-process coordination
//! - L1 fallback

use crate::error::Result;
use crate::rate_limit::RateLimitState;

const KIND_RATE_LIMIT: &str = "EnvoyRateLimit";
const KIND_RATE_LIMIT_BAN: &str = "EnvoyRateLimitBan";

/// Persistent store for rate limit state.
///
/// Follows AgentRegistry pattern: write-through to sqlitegraph on mutations,
/// load from DB on startup.
pub struct RateLimitStore;

impl RateLimitStore {
    pub fn new() -> Self {
        Self
    }

    /// Persist rate limit state to sqlitegraph.
    pub fn persist(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        state: &RateLimitState,
    ) -> Result<()> {
        use sqlitegraph::GraphEntity;

        let data = serde_json::json!({
            "tokens": state.bucket().tokens,
            "max_tokens": state.bucket().max_tokens,
            "replenish_rate": state.bucket().replenish_rate,
        });

        if let Some(mut entity) =
            graph.find_entity_by_kind_and_name(KIND_RATE_LIMIT, &state.agent_id)?
        {
            entity.data = data;
            graph.update_entity(&entity)?;
        } else {
            let entity = GraphEntity {
                id: 0,
                kind: KIND_RATE_LIMIT.to_string(),
                name: state.agent_id.clone(),
                file_path: None,
                data,
            };
            graph.insert_entity(&entity)?;
        }
        Ok(())
    }

    /// Load rate limit state from sqlitegraph.
    pub fn load(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
    ) -> Result<Option<RateLimitState>> {
        use crate::rate_limit::TokenBucket;

        if let Some(entity) =
            graph.find_entity_by_kind_and_name(KIND_RATE_LIMIT, agent_id)?
        {
            let tokens = entity
                .data
                .get("tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let max_tokens = entity
                .data
                .get("max_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(1000);
            let replenish_rate = entity
                .data
                .get("replenish_rate")
                .and_then(|v| v.as_u64())
                .unwrap_or(100);

            let bucket = TokenBucket {
                tokens,
                max_tokens,
                replenish_rate,
                last_replenish: std::time::Instant::now(),
            };

            return Ok(Some(RateLimitState::from_bucket(
                agent_id.to_string(),
                bucket,
            )));
        }
        Ok(None)
    }

    /// Load all rate limit states from sqlitegraph.
    pub fn load_all(
        &self,
        graph: &sqlitegraph::SqliteGraph,
    ) -> Result<Vec<RateLimitState>> {
        let entities = graph.find_entities_by_kind(KIND_RATE_LIMIT)?;
        let mut states = Vec::new();

        for entity in &entities {
            if let Some(state) = self.load(graph, &entity.name)? {
                states.push(state);
            }
        }

        Ok(states)
    }

    /// Persist a ban to sqlitegraph.
    pub fn persist_ban(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
        reason: &str,
    ) -> Result<()> {
        use sqlitegraph::GraphEntity;

        let data = serde_json::json!({
            "reason": reason,
            "banned_at": chrono::Utc::now().to_rfc3339(),
        });

        if let Some(mut entity) =
            graph.find_entity_by_kind_and_name(KIND_RATE_LIMIT_BAN, agent_id)?
        {
            entity.data = data;
            graph.update_entity(&entity)?;
        } else {
            let entity = GraphEntity {
                id: 0,
                kind: KIND_RATE_LIMIT_BAN.to_string(),
                name: agent_id.to_string(),
                file_path: None,
                data,
            };
            graph.insert_entity(&entity)?;
        }
        Ok(())
    }

    /// Remove a ban from sqlitegraph.
    pub fn remove_ban(&self, graph: &sqlitegraph::SqliteGraph, agent_id: &str) -> Result<()> {
        if let Some(entity) =
            graph.find_entity_by_kind_and_name(KIND_RATE_LIMIT_BAN, agent_id)?
        {
            graph.delete_entity(entity.id)?;
        }
        Ok(())
    }

    /// Load all bans from sqlitegraph.
    pub fn load_bans(
        &self,
        graph: &sqlitegraph::SqliteGraph,
    ) -> Result<std::collections::HashMap<String, String>> {
        let entities = graph.find_entities_by_kind(KIND_RATE_LIMIT_BAN)?;
        let mut bans = std::collections::HashMap::new();

        for entity in &entities {
            if let Some(reason) = entity.data.get("reason").and_then(|v| v.as_str()) {
                bans.insert(entity.name.clone(), reason.to_string());
            }
        }

        Ok(bans)
    }
}

impl Default for RateLimitStore {
    fn default() -> Self {
        Self::new()
    }
}
