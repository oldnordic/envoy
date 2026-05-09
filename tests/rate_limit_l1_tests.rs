//! Hybrid rate limiter tests — L1 AHashMap cache.
//! Tests written FIRST (TDD), will fail until implementation exists.

use envoy::rate_limit::{HybridRateLimiter, RateLimitConfig};
use std::time::Duration;

#[test]
fn test_l1_hit_ratio_under_load() {
    // Given: A hybrid rate limiter with L1 capacity 100
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let config = RateLimitConfig::new(100, 10, 500);
    let limiter = HybridRateLimiter::new(&graph, config, 100).unwrap();

    // When: Check rate limit for same agent 100 times
    for _ in 0..100 {
        let decision = limiter.check_rate_limit(&graph, "test-agent");
        assert!(decision.allowed);
    }

    // Then: Should have >90% hit ratio (all hits in this case)
    let stats = limiter.stats();
    assert!(stats.l1_hits >= 90);
}

#[test]
fn test_lru_eviction_flushes_to_l2() {
    // Given: A hybrid rate limiter with L1 capacity 3
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let config = RateLimitConfig::new(100, 10, 500);
    let limiter = HybridRateLimiter::new(&graph, config, 3).unwrap();

    // When: Access 4 different agents (causes eviction)
    limiter.check_rate_limit(&graph, "agent1");
    limiter.check_rate_limit(&graph, "agent2");
    limiter.check_rate_limit(&graph, "agent3");
    limiter.check_rate_limit(&graph, "agent4"); // Evicts agent1

    // Then: agent1 state should be in L2 (sqlitegraph)
    let store = limiter.store();
    let loaded = store.load(&graph, "agent1").unwrap();
    assert!(loaded.is_some());
}

#[test]
fn test_l1_miss_loads_from_l2() {
    // Given: A database with a persisted state
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let store = envoy::rate_limit::RateLimitStore::new();

    let mut state = envoy::rate_limit::RateLimitState::new("cached-agent", 50, 10);
    state.check(25); // 25 tokens remaining
    store.persist(&graph, &state).unwrap();

    // When: Create limiter (loads L2 into L1 during init)
    let config = RateLimitConfig::new(100, 10, 500);
    let limiter = HybridRateLimiter::new(&graph, config, 100).unwrap();

    // Clear L1 cache to force L2 lookup on next call
    limiter.clear_l1_for_testing();
    let decision = limiter.check_rate_limit(&graph, "cached-agent");

    // Then: Should allow (25 tokens remaining)
    assert!(decision.allowed);

    // And: Should have recorded an L1 miss
    let stats = limiter.stats();
    assert!(stats.l1_misses > 0);
}

#[test]
fn test_rate_limit_blocks_over_quota() {
    // Given: A hybrid rate limiter
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let config = RateLimitConfig::new(10, 1, 50);
    let limiter = HybridRateLimiter::new(&graph, config, 100).unwrap();

    // When: Consume all tokens
    for _ in 0..10 {
        assert!(limiter.check_rate_limit(&graph, "greedy-agent").allowed);
    }

    // Then: Next request should be blocked
    let decision = limiter.check_rate_limit(&graph, "greedy-agent");
    assert!(!decision.allowed);
    assert!(decision.retry_after.is_some());
}

#[test]
fn test_replenish_over_time() {
    // Given: A hybrid rate limiter with replenish rate 100/sec
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let config = RateLimitConfig::new(1000, 100, 5000);
    let limiter = HybridRateLimiter::new(&graph, config, 100).unwrap();

    // When: Consume all tokens, wait 1 second
    for _ in 0..1000 {
        limiter.check_rate_limit(&graph, "test-agent");
    }
    assert!(!limiter.check_rate_limit(&graph, "test-agent").allowed);

    // Simulate time passing (replenish)
    limiter.replenish_all(Duration::from_secs(1));

    // Then: Should have tokens again
    assert!(limiter.check_rate_limit(&graph, "test-agent").allowed);
}

#[test]
fn test_ban_agent() {
    // Given: A hybrid rate limiter
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let config = RateLimitConfig::default();
    let limiter = HybridRateLimiter::new(&graph, config, 100).unwrap();

    // When: Ban an agent
    limiter
        .ban_agent(&graph, "malicious-agent", "spamming")
        .unwrap();

    // Then: All requests should be blocked
    let decision = limiter.check_rate_limit(&graph, "malicious-agent");
    assert!(!decision.allowed);
}

#[test]
fn test_stats_endpoint() {
    // Given: A hybrid rate limiter
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let config = RateLimitConfig::default();
    let limiter = HybridRateLimiter::new(&graph, config, 100).unwrap();

    // When: Generate some activity
    limiter.check_rate_limit(&graph, "agent1");
    limiter.check_rate_limit(&graph, "agent2");
    limiter.check_rate_limit(&graph, "agent1"); // L1 hit
    limiter.check_rate_limit(&graph, "agent2"); // L1 hit
    limiter.check_rate_limit(&graph, "agent1"); // L1 hit

    // Then: Stats should reflect activity
    let stats = limiter.stats();
    assert_eq!(stats.l1_size, 2);
    // We have 5 total calls, first 2 are misses (load from L2), rest are hits
    assert_eq!(stats.l1_hits, 3);
    assert_eq!(stats.l1_misses, 2);
}

#[test]
fn test_ahash_compiles() {
    // This test verifies ahash compiles and is usable.
    // Actual benchmarking happens in Phase 6.

    use ahash::AHashMap;
    use std::collections::HashMap;

    // AHashMap should work
    let mut a_map: AHashMap<String, u64> = AHashMap::new();
    a_map.insert("test".to_string(), 42);
    assert_eq!(a_map.get("test"), Some(&42));

    // std HashMap should also work (for comparison)
    let mut std_map: HashMap<String, u64> = HashMap::new();
    std_map.insert("test".to_string(), 42);
    assert_eq!(std_map.get("test"), Some(&42));
}
