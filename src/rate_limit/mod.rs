//! Hybrid rate limiter — L1 in-memory (AHashMap) + L2 (sqlitegraph) persistence.
//!
//! Per-agent token buckets with SIMD-optimized lookups (ahash) for high throughput.
//! Falls back to sqlitegraph for L1 misses and persistence across restarts.
//!
//! Follows AgentRegistry pattern: L1 in-memory cache, write-through to sqlitegraph,
//! load from DB on startup. Graph is passed as a parameter to methods that need it.

pub mod config;
pub mod state;
pub mod store;

pub use config::RateLimitConfig;
pub use state::{RateLimitDecision, RateLimitState, TokenBucket};
pub use store::RateLimitStore;

use ahash::AHashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Statistics for the hybrid rate limiter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridRateLimiterStats {
    pub l1_size: usize,
    pub l1_capacity: usize,
    pub l1_hits: u64,
    pub l1_misses: u64,
    pub l1_evictions: u64,
    pub banned_agents: Vec<String>,
}

/// Per-agent state with LRU tracking.
#[derive(Debug, Clone)]
struct L1Entry {
    state: RateLimitState,
    last_accessed: std::time::Instant,
}

/// Hybrid rate limiter with L1 (AHashMap) + L2 (sqlitegraph).
///
/// L1: In-memory cache using ahash::AHashMap for fast lookups.
/// L2: sqlitegraph persistence for durability and L1 misses.
///
/// Follows AgentRegistry pattern: methods take `&sqlitegraph::SqliteGraph` parameter
/// for L2 operations, rather than storing the graph internally.
pub struct HybridRateLimiter {
    store: RateLimitStore,
    config: RateLimitConfig,
    l1: Arc<RwLock<AHashMap<String, L1Entry>>>,
    l1_capacity: usize,
    banned: Arc<RwLock<HashMap<String, String>>>, // agent_id -> reason
    stats: Arc<RwLock<HybridRateLimiterStats>>,
}

impl HybridRateLimiter {
    /// Create a new hybrid rate limiter.
    ///
    /// # Arguments
    /// * `graph` — sqlitegraph database for L2 persistence (used during initialization)
    /// * `config` — Rate limit configuration
    /// * `l1_capacity` — Maximum number of agents to cache in L1
    pub fn new(
        graph: &sqlitegraph::SqliteGraph,
        config: RateLimitConfig,
        l1_capacity: usize,
    ) -> crate::error::Result<Self> {
        let store = RateLimitStore::new();

        // Load existing states from L2 into L1 (cache warming)
        let mut l1_map = AHashMap::new();
        if let Ok(states) = store.load_all(graph) {
            for state in states {
                l1_map.insert(
                    state.agent_id.clone(),
                    L1Entry {
                        state,
                        last_accessed: std::time::Instant::now(),
                    },
                );
            }
        }
        let l1 = Arc::new(RwLock::new(l1_map));

        let stats = HybridRateLimiterStats {
            l1_size: l1.read().len(),
            l1_capacity,
            l1_hits: 0,
            l1_misses: 0,
            l1_evictions: 0,
            banned_agents: Vec::new(),
        };

        Ok(Self {
            store,
            config,
            l1,
            l1_capacity,
            banned: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(stats)),
        })
    }

    /// Check if a request is allowed for the given agent.
    ///
    /// Takes graph as parameter for L2 fallback.
    pub fn check_rate_limit(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
    ) -> RateLimitDecision {
        // Check ban list first
        if self.banned.read().contains_key(agent_id) {
            return RateLimitDecision {
                allowed: false,
                retry_after: Some(Duration::from_secs(3600)), // 1 hour
            };
        }

        // Try L1 first
        {
            let mut l1 = self.l1.write();
            if let Some(entry) = l1.get_mut(agent_id) {
                entry.last_accessed = std::time::Instant::now();
                self.stats.write().l1_hits += 1;
                return entry.state.check(1);
            }
        }

        // L1 miss — try L2
        self.stats.write().l1_misses += 1;
        if let Ok(Some(state)) = self.store.load(graph, agent_id) {
            // Promote to L1
            let mut l1 = self.l1.write();
            if l1.len() >= self.l1_capacity {
                self.evict_lru(graph, &mut l1);
            }
            l1.insert(
                agent_id.to_string(),
                L1Entry {
                    state,
                    last_accessed: std::time::Instant::now(),
                },
            );
            self.stats.write().l1_size = l1.len();

            // Check the newly loaded state
            if let Some(entry) = l1.get_mut(agent_id) {
                return entry.state.check(1);
            }
        }

        // Create new state
        let mut state =
            RateLimitState::new(agent_id, self.config.max_tokens, self.config.replenish_rate);
        let decision = state.check(1);

        let mut l1 = self.l1.write();
        if l1.len() >= self.l1_capacity {
            self.evict_lru(graph, &mut l1);
        }
        l1.insert(
            agent_id.to_string(),
            L1Entry {
                state,
                last_accessed: std::time::Instant::now(),
            },
        );
        self.stats.write().l1_size = l1.len();

        decision
    }

    /// Evict the least-recently-used entry from L1, flushing to L2.
    fn evict_lru(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        l1: &mut AHashMap<String, L1Entry>,
    ) {
        let lru_key = l1
            .iter()
            .min_by_key(|(_, v)| v.last_accessed)
            .map(|(k, _)| k.clone());

        if let Some(key) = lru_key {
            if let Some(entry) = l1.remove(&key) {
                // Flush to L2
                let _ = self.store.persist(graph, &entry.state);
                self.stats.write().l1_evictions += 1;
            }
        }
    }

    /// Replenish tokens for all cached agents.
    pub fn replenish_all(&self, elapsed: Duration) {
        let mut l1 = self.l1.write();
        for entry in l1.values_mut() {
            entry.state.replenish(elapsed);
        }
    }

    /// Ban an agent (permanent until unbanned).
    pub fn ban_agent(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
        reason: &str,
    ) -> crate::error::Result<()> {
        self.banned
            .write()
            .insert(agent_id.to_string(), reason.to_string());

        // Remove from L1 and persist ban to L2
        self.l1.write().remove(agent_id);
        let _ = self.store.persist_ban(graph, agent_id, reason);

        // Update stats
        let mut stats = self.stats.write();
        stats.banned_agents = self.banned.read().keys().cloned().collect();
        stats.l1_size = self.l1.read().len();

        Ok(())
    }

    /// Unban an agent.
    pub fn unban_agent(
        &self,
        graph: &sqlitegraph::SqliteGraph,
        agent_id: &str,
    ) -> crate::error::Result<()> {
        self.banned.write().remove(agent_id);
        let _ = self.store.remove_ban(graph, agent_id);

        // Update stats
        let mut stats = self.stats.write();
        stats.banned_agents = self.banned.read().keys().cloned().collect();

        Ok(())
    }

    /// Get current statistics.
    pub fn stats(&self) -> HybridRateLimiterStats {
        self.stats.read().clone()
    }

    /// Access the underlying store (for testing).
    pub fn store(&self) -> &RateLimitStore {
        &self.store
    }

    /// Clear L1 cache (for testing only).
    #[doc(hidden)]
    pub fn clear_l1_for_testing(&self) {
        self.l1.write().clear();
        self.stats.write().l1_size = 0;
    }
}
