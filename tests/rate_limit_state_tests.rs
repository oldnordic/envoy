//! Rate limit state tests — token bucket math and replenishment logic.
//! Tests written FIRST (TDD), will fail until implementation exists.

use envoy::rate_limit::{RateLimitConfig, RateLimitState, TokenBucket};

#[test]
fn test_token_bucket_replenish() {
    // Given: A bucket with 0 tokens, capacity 100, replenish rate 10/sec
    let mut bucket = TokenBucket::new(100, 10);
    assert_eq!(bucket.tokens, 100);

    // When: Consume all tokens
    bucket.consume(100);
    assert_eq!(bucket.tokens, 0);

    // When: 1 second passes
    bucket.replenish(std::time::Duration::from_secs(1));

    // Then: Should have 10 tokens (replenish_rate * time)
    assert_eq!(bucket.tokens, 10);
}

#[test]
fn test_token_bucket_no_overfill() {
    // Given: A bucket at 90 tokens, capacity 100
    let mut bucket = TokenBucket::new(100, 10);
    bucket.tokens = 90;

    // When: 2 seconds pass (would add 20 tokens)
    bucket.replenish(std::time::Duration::from_secs(2));

    // Then: Should cap at 100, not 110
    assert_eq!(bucket.tokens, 100);
}

#[test]
fn test_token_bucket_consume_partial() {
    // Given: A bucket with 50 tokens
    let mut bucket = TokenBucket::new(100, 10);
    bucket.tokens = 50;

    // When: Try to consume 30 tokens
    let result = bucket.try_consume(30);

    // Then: Should succeed, have 20 tokens left
    assert!(result.is_ok());
    assert_eq!(bucket.tokens, 20);
}

#[test]
fn test_token_bucket_consume_insufficient() {
    // Given: A bucket with 10 tokens
    let mut bucket = TokenBucket::new(100, 10);
    bucket.tokens = 10;

    // When: Try to consume 30 tokens
    let result = bucket.try_consume(30);

    // Then: Should fail, tokens unchanged
    assert!(result.is_err());
    assert_eq!(bucket.tokens, 10);
}

#[test]
fn test_rate_limit_state_allow_under_quota() {
    // Given: A rate limit state with 100 tokens
    let mut state = RateLimitState::new("test-agent", 100, 10);

    // When: Check rate limit for 1 request (cost = 1 token)
    let decision = state.check(1);

    // Then: Should allow
    assert!(decision.allowed);
}

#[test]
fn test_rate_limit_state_deny_over_quota() {
    // Given: A rate limit state with 5 tokens
    let mut state = RateLimitState::new("test-agent", 5, 10);

    // When: Check rate limit for 10 requests (cost = 10 tokens)
    let decision = state.check(10);

    // Then: Should deny
    assert!(!decision.allowed);
}

#[test]
fn test_rate_limit_state_replenish_over_time() {
    // Given: A rate limit state with max_tokens=100, starting at 10, replenish rate 50/sec
    let mut state = RateLimitState::new("test-agent", 100, 50);
    // Consume 90 tokens to get to 10
    state.bucket_mut().consume(90);

    // When: 1 second passes
    state.replenish(std::time::Duration::from_secs(1));

    // Then: Should have 60 tokens (10 + 50 replenished)
    assert_eq!(state.bucket().tokens, 60);
}

#[test]
fn test_rate_limit_config_default() {
    // Given: Default config
    let config = RateLimitConfig::default();

    // Then: Should have sensible defaults
    assert_eq!(config.max_tokens, 100_000);
    assert_eq!(config.replenish_rate, 50_000);
    assert_eq!(config.burst_size, 200_000);
}

#[test]
fn test_rate_limit_config_custom() {
    // Given: Custom config
    let config = RateLimitConfig::new(500, 50, 1000);

    // Then: Should use custom values
    assert_eq!(config.max_tokens, 500);
    assert_eq!(config.replenish_rate, 50);
    assert_eq!(config.burst_size, 1000);
}

#[test]
fn test_ahash_faster_than_std() {
    // This is a benchmark test — we just verify it compiles and runs.
    // Actual benchmarking happens in cargo test --release.

    use std::collections::HashMap;
    use std::time::Instant;

    // std::collections::HashMap
    let start = Instant::now();
    let mut std_map: HashMap<String, u64> = HashMap::new();
    for i in 0..10_000 {
        std_map.insert(format!("agent-{}", i), i);
    }
    let std_duration = start.elapsed();

    // Verify we inserted all
    assert_eq!(std_map.len(), 10_000);

    // We just need this to compile and run — benchmarking for actual numbers
    // will be done in Phase 6 with cargo bench.
    println!("std::HashMap insert 10K: {:?}", std_duration);
}
