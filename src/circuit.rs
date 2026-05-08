use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Per-agent circuit breaker state.
#[derive(Debug, Clone)]
enum CircuitState {
    Closed {
        failures: u32,
    },
    Open {
        opened_at: DateTime<Utc>,
        failures: u32,
    },
    HalfOpen {
        failures: u32,
    },
}

/// Whether delivery is allowed for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanDeliver {
    Yes,
    No,
    Probe,
}

/// Snapshot of circuit state for status queries.
#[derive(Debug, Clone, Serialize)]
pub struct CircuitStatus {
    pub agent_id: String,
    pub state: String,
    pub failures: u32,
    pub opened_at: Option<String>,
}

/// Configuration for the circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitConfig {
    pub failure_threshold: u32,
    pub cooldown_seconds: u64,
}

impl Default for CircuitConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            cooldown_seconds: 60,
        }
    }
}

/// Per-agent circuit breaker for WS delivery.
///
/// Tracks delivery failures per agent and prevents hammering dead endpoints.
/// Heartbeat from an agent resets its circuit to Closed (agent is alive).
pub struct CircuitBreaker {
    states: Arc<Mutex<HashMap<String, CircuitState>>>,
    config: CircuitConfig,
}

impl CircuitBreaker {
    pub fn new(config: CircuitConfig) -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(CircuitConfig::default())
    }

    /// Check if delivery is allowed for this agent.
    pub fn check(&self, agent_id: &str) -> CanDeliver {
        let mut states = self.states.lock().unwrap();
        let state = states
            .entry(agent_id.to_string())
            .or_insert_with(|| CircuitState::Closed { failures: 0 });

        match state {
            CircuitState::Closed { .. } => CanDeliver::Yes,
            CircuitState::Open { opened_at, .. } => {
                let elapsed = (Utc::now() - *opened_at).num_seconds();
                if elapsed >= self.config.cooldown_seconds as i64 {
                    let failures = match state {
                        CircuitState::Open { failures, .. } => *failures,
                        _ => 0,
                    };
                    *state = CircuitState::HalfOpen { failures };
                    CanDeliver::Probe
                } else {
                    CanDeliver::No
                }
            }
            CircuitState::HalfOpen { .. } => CanDeliver::Probe,
        }
    }

    /// Record a delivery failure. May transition Closed -> Open.
    pub fn record_failure(&self, agent_id: &str) {
        let mut states = self.states.lock().unwrap();
        let state = states
            .entry(agent_id.to_string())
            .or_insert_with(|| CircuitState::Closed { failures: 0 });

        match state {
            CircuitState::Closed { failures } => {
                let new_failures = *failures + 1;
                if new_failures >= self.config.failure_threshold {
                    *state = CircuitState::Open {
                        opened_at: Utc::now(),
                        failures: new_failures,
                    };
                } else {
                    *state = CircuitState::Closed {
                        failures: new_failures,
                    };
                }
            }
            CircuitState::Open { failures, .. } => {
                *state = CircuitState::Open {
                    opened_at: Utc::now(),
                    failures: *failures + 1,
                };
            }
            CircuitState::HalfOpen { failures } => {
                *state = CircuitState::Open {
                    opened_at: Utc::now(),
                    failures: *failures + 1,
                };
            }
        }
    }

    /// Record a delivery success. Transitions HalfOpen -> Closed.
    pub fn record_success(&self, agent_id: &str) {
        let mut states = self.states.lock().unwrap();
        if let Some(state) = states.get_mut(agent_id) {
            if matches!(state, CircuitState::HalfOpen { .. }) {
                *state = CircuitState::Closed { failures: 0 };
            }
        }
    }

    /// Reset circuit on heartbeat. Any state -> Closed.
    pub fn reset(&self, agent_id: &str) {
        let mut states = self.states.lock().unwrap();
        states.insert(agent_id.to_string(), CircuitState::Closed { failures: 0 });
    }

    /// Remove circuit state for an agent (on disconnect).
    pub fn remove(&self, agent_id: &str) {
        let mut states = self.states.lock().unwrap();
        states.remove(agent_id);
    }

    /// Evict entries that have been Open for longer than 1 hour.
    /// Returns the number of evicted entries.
    pub fn evict_stale(&self) -> usize {
        let mut states = self.states.lock().unwrap();
        let cutoff = Utc::now() - chrono::Duration::hours(1);
        let before = states.len();
        states.retain(|_, state| match state {
            CircuitState::Open { opened_at, .. } => *opened_at > cutoff,
            _ => true,
        });
        before - states.len()
    }

    /// Get current state for status queries.
    pub fn get_state(&self, agent_id: &str) -> CircuitStatus {
        let states = self.states.lock().unwrap();
        match states.get(agent_id) {
            None => CircuitStatus {
                agent_id: agent_id.to_string(),
                state: "closed".to_string(),
                failures: 0,
                opened_at: None,
            },
            Some(CircuitState::Closed { failures }) => CircuitStatus {
                agent_id: agent_id.to_string(),
                state: "closed".to_string(),
                failures: *failures,
                opened_at: None,
            },
            Some(CircuitState::Open {
                opened_at,
                failures,
            }) => CircuitStatus {
                agent_id: agent_id.to_string(),
                state: "open".to_string(),
                failures: *failures,
                opened_at: Some(opened_at.to_rfc3339()),
            },
            Some(CircuitState::HalfOpen { failures }) => CircuitStatus {
                agent_id: agent_id.to_string(),
                state: "half_open".to_string(),
                failures: *failures,
                opened_at: None,
            },
        }
    }

    /// List all circuits with non-default state.
    pub fn list_active(&self) -> Vec<CircuitStatus> {
        let states = self.states.lock().unwrap();
        let mut result = Vec::new();
        for (agent_id, state) in states.iter() {
            match state {
                CircuitState::Closed { failures: 0 } => continue,
                CircuitState::Closed { failures } => result.push(CircuitStatus {
                    agent_id: agent_id.clone(),
                    state: "closed".to_string(),
                    failures: *failures,
                    opened_at: None,
                }),
                CircuitState::Open {
                    opened_at,
                    failures,
                } => result.push(CircuitStatus {
                    agent_id: agent_id.clone(),
                    state: "open".to_string(),
                    failures: *failures,
                    opened_at: Some(opened_at.to_rfc3339()),
                }),
                CircuitState::HalfOpen { failures } => result.push(CircuitStatus {
                    agent_id: agent_id.clone(),
                    state: "half_open".to_string(),
                    failures: *failures,
                    opened_at: None,
                }),
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_breaker() -> CircuitBreaker {
        CircuitBreaker::new(CircuitConfig {
            failure_threshold: 3,
            cooldown_seconds: 60,
        })
    }

    fn breaker_with_cooldown() -> CircuitBreaker {
        CircuitBreaker::new(CircuitConfig {
            failure_threshold: 3,
            cooldown_seconds: 60,
        })
    }

    #[test]
    fn starts_closed() {
        let cb = test_breaker();
        assert_eq!(cb.check("agent1"), CanDeliver::Yes);
    }

    #[test]
    fn stays_closed_below_threshold() {
        let cb = test_breaker();
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::Yes);
    }

    #[test]
    fn opens_at_threshold() {
        let cb = test_breaker();
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::No);
        let status = cb.get_state("agent1");
        assert_eq!(status.state, "open");
    }

    #[test]
    fn half_open_after_cooldown() {
        let cb = breaker_with_cooldown();
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::No);

        // Manually transition to expired open state
        {
            let mut states = cb.states.lock().unwrap();
            states.insert(
                "agent1".to_string(),
                CircuitState::Open {
                    opened_at: Utc::now() - chrono::Duration::seconds(120),
                    failures: 3,
                },
            );
        }
        assert_eq!(cb.check("agent1"), CanDeliver::Probe);
    }

    #[test]
    fn half_open_success_closes() {
        let cb = test_breaker();
        // Force half-open
        {
            let mut states = cb.states.lock().unwrap();
            states.insert("agent1".to_string(), CircuitState::HalfOpen { failures: 3 });
        }
        cb.record_success("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::Yes);
        let status = cb.get_state("agent1");
        assert_eq!(status.failures, 0);
    }

    #[test]
    fn half_open_failure_reopens() {
        let cb = breaker_with_cooldown();
        {
            let mut states = cb.states.lock().unwrap();
            states.insert("agent1".to_string(), CircuitState::HalfOpen { failures: 3 });
        }
        cb.record_failure("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::No);
    }

    #[test]
    fn heartbeat_resets_closed() {
        let cb = test_breaker();
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::No);

        cb.reset("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::Yes);
        assert_eq!(cb.get_state("agent1").failures, 0);
    }

    #[test]
    fn heartbeat_resets_open() {
        let cb = breaker_with_cooldown();
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::No);

        cb.reset("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::Yes);
    }

    #[test]
    fn heartbeat_resets_half_open() {
        let cb = breaker_with_cooldown();
        {
            let mut states = cb.states.lock().unwrap();
            states.insert("agent1".to_string(), CircuitState::HalfOpen { failures: 3 });
        }
        cb.reset("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::Yes);
    }

    #[test]
    fn different_agents_independent() {
        let cb = test_breaker();
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::No);
        assert_eq!(cb.check("agent2"), CanDeliver::Yes);
    }

    #[test]
    fn remove_clears_state() {
        let cb = test_breaker();
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        cb.remove("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::Yes);
    }

    #[test]
    fn list_active_skips_healthy() {
        let cb = test_breaker();
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        cb.record_failure("agent1");
        let active = cb.list_active();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].agent_id, "agent1");
    }

    #[test]
    fn full_lifecycle() {
        let cb = breaker_with_cooldown();

        // Start closed
        assert_eq!(cb.check("agent1"), CanDeliver::Yes);

        // Accumulate failures
        for _ in 0..3 {
            cb.record_failure("agent1");
        }
        assert_eq!(cb.check("agent1"), CanDeliver::No);

        // Expire cooldown -> half-open
        {
            let mut states = cb.states.lock().unwrap();
            states.insert(
                "agent1".to_string(),
                CircuitState::Open {
                    opened_at: Utc::now() - chrono::Duration::seconds(120),
                    failures: 3,
                },
            );
        }
        assert_eq!(cb.check("agent1"), CanDeliver::Probe);

        // Probe succeeds -> closed
        cb.record_success("agent1");
        assert_eq!(cb.check("agent1"), CanDeliver::Yes);
        assert_eq!(cb.get_state("agent1").failures, 0);
    }
}
