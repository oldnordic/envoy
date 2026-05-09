//! Rate limit configuration.

use serde::{Deserialize, Serialize};

/// Rate limit configuration.
///
/// Can be loaded from config file or constructed programmatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum tokens per agent (capacity).
    pub max_tokens: u64,

    /// Tokens replenished per second.
    pub replenish_rate: u64,

    /// Burst capacity (short-term allowance above max_tokens).
    pub burst_size: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_tokens: 1000,
            replenish_rate: 100,
            burst_size: 5000,
        }
    }
}

impl RateLimitConfig {
    pub fn new(max_tokens: u64, replenish_rate: u64, burst_size: u64) -> Self {
        Self {
            max_tokens,
            replenish_rate,
            burst_size,
        }
    }
}
