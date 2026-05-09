//! Rate limit store tests — sqlitegraph persistence.
//! Tests written FIRST (TDD), will fail until full implementation exists.

use envoy::rate_limit::{RateLimitStore, RateLimitState};

#[test]
fn test_persist_rate_limit() {
    // Given: An in-memory database and store
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let store = RateLimitStore::new();

    // When: Persist a rate limit state
    let state = RateLimitState::new("test-agent", 100, 10);
    store.persist(&graph, &state).unwrap();

    // Then: Should be able to load it back
    let loaded = store.load(&graph, "test-agent").unwrap();
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.agent_id, "test-agent");
    assert_eq!(loaded.bucket().tokens, 100);
    assert_eq!(loaded.bucket().max_tokens, 100);
    assert_eq!(loaded.bucket().replenish_rate, 10);
}

#[test]
fn test_load_all_on_startup() {
    // Given: A database with multiple rate limit states
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let store = RateLimitStore::new();

    store.persist(&graph, &RateLimitState::new("agent1", 100, 10)).unwrap();
    store.persist(&graph, &RateLimitState::new("agent2", 200, 20)).unwrap();
    store.persist(&graph, &RateLimitState::new("agent3", 300, 30)).unwrap();

    // When: Load all states
    let states = store.load_all(&graph).unwrap();

    // Then: Should load all 3
    assert_eq!(states.len(), 3);
    let agent_ids: Vec<_> = states.iter().map(|s| s.agent_id.as_str()).collect();
    assert!(agent_ids.contains(&"agent1"));
    assert!(agent_ids.contains(&"agent2"));
    assert!(agent_ids.contains(&"agent3"));
}

#[test]
fn test_update_existing_state() {
    // Given: A database with an existing rate limit state
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let store = RateLimitStore::new();

    let mut state = RateLimitState::new("test-agent", 100, 10);
    store.persist(&graph, &state).unwrap();

    // When: Consume tokens and persist again
    state.check(50); // Consume 50 tokens
    store.persist(&graph, &state).unwrap();

    // Then: Should load updated state
    let loaded = store.load(&graph, "test-agent").unwrap().unwrap();
    assert_eq!(loaded.bucket().tokens, 50);
}

#[test]
fn test_load_nonexistent_returns_none() {
    // Given: An empty database
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let store = RateLimitStore::new();

    // When: Load a nonexistent agent
    let loaded = store.load(&graph, "nonexistent").unwrap();

    // Then: Should return None
    assert!(loaded.is_none());
}

#[test]
fn test_persist_with_zero_tokens() {
    // Given: A rate limit state with 0 tokens (exhausted)
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let store = RateLimitStore::new();

    let mut state = RateLimitState::new("exhausted-agent", 100, 10);
    state.check(100); // Consume all tokens

    // When: Persist
    store.persist(&graph, &state).unwrap();

    // Then: Should load with 0 tokens
    let loaded = store.load(&graph, "exhausted-agent").unwrap().unwrap();
    assert_eq!(loaded.bucket().tokens, 0);
}

#[test]
fn test_entity_kind_is_correct() {
    // Given: A database with a persisted state
    let graph = sqlitegraph::SqliteGraph::open_in_memory().unwrap();
    let store = RateLimitStore::new();

    store.persist(&graph, &RateLimitState::new("test-agent", 100, 10)).unwrap();

    // When: Query entities by kind
    let entities = graph.find_entities_by_kind("EnvoyRateLimit").unwrap();

    // Then: Should have exactly 1 entity
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].name, "test-agent");
    assert_eq!(entities[0].kind, "EnvoyRateLimit");
}
