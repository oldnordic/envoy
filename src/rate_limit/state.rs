//! Rate limit state types — token bucket, configuration, per-agent state.

use std::time::Duration;

/// Token bucket for rate limiting.
///
/// Tokens replenish over time based on `replenish_rate` (tokens/second).
/// Capacity is capped at `max_tokens`.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    pub tokens: u64,
    pub max_tokens: u64,
    pub replenish_rate: u64,
    pub last_replenish: std::time::Instant,
}

impl TokenBucket {
    pub fn new(max_tokens: u64, replenish_rate: u64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            replenish_rate,
            last_replenish: std::time::Instant::now(),
        }
    }

    /// Replenish tokens based on elapsed time.
    pub fn replenish(&mut self, elapsed: Duration) {
        let secs = elapsed.as_secs_f64();
        let tokens_to_add = (secs * self.replenish_rate as f64) as u64;
        self.tokens = (self.tokens + tokens_to_add).min(self.max_tokens);
        self.last_replenish = std::time::Instant::now();
    }

    /// Consume tokens if available.
    pub fn consume(&mut self, amount: u64) {
        self.tokens = self.tokens.saturating_sub(amount);
    }

    /// Try to consume tokens, returning error if insufficient.
    pub fn try_consume(&mut self, amount: u64) -> Result<(), u64> {
        if self.tokens >= amount {
            self.tokens -= amount;
            Ok(())
        } else {
            Err(self.tokens)
        }
    }
}

/// Rate limit decision result.
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitDecision {
    pub allowed: bool,
    pub retry_after: Option<Duration>,
}

/// Per-agent rate limit state.
#[derive(Debug, Clone)]
pub struct RateLimitState {
    pub agent_id: String,
    bucket: TokenBucket,
}

impl RateLimitState {
    pub fn new(agent_id: &str, max_tokens: u64, replenish_rate: u64) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            bucket: TokenBucket::new(max_tokens, replenish_rate),
        }
    }

    /// Create from an existing bucket (used by RateLimitStore).
    pub(crate) fn from_bucket(agent_id: String, bucket: TokenBucket) -> Self {
        Self { agent_id, bucket }
    }

    /// Check if a request is allowed, consuming tokens if so.
    pub fn check(&mut self, cost: u64) -> RateLimitDecision {
        if self.bucket.tokens >= cost {
            self.bucket.consume(cost);
            RateLimitDecision {
                allowed: true,
                retry_after: None,
            }
        } else {
            let deficit = cost - self.bucket.tokens;
            let retry_after = Duration::from_secs_f64(
                deficit as f64 / self.bucket.replenish_rate as f64,
            );
            RateLimitDecision {
                allowed: false,
                retry_after: Some(retry_after),
            }
        }
    }

    /// Replenish tokens based on elapsed time.
    pub fn replenish(&mut self, elapsed: Duration) {
        self.bucket.replenish(elapsed);
    }

    /// Access the underlying bucket (for testing).
    pub fn bucket(&self) -> &TokenBucket {
        &self.bucket
    }

    /// Access the underlying bucket mutably (for testing).
    pub fn bucket_mut(&mut self) -> &mut TokenBucket {
        &mut self.bucket
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_initial_state() {
        let bucket = TokenBucket::new(100, 10);
        assert_eq!(bucket.tokens, 100);
        assert_eq!(bucket.max_tokens, 100);
        assert_eq!(bucket.replenish_rate, 10);
    }
}
