//! # Envoy — Message/Coordination Server for AI Coding Agents
//!
//! Event-driven pub/sub coordination built on SQLite.
//! Replaces file-based message passing with an append-only event log.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use envoy::{Engine, EventPayload, AgentStatus};
//!
//! let engine = Engine::open("coordination.db")?;
//!
//! // Create a channel for agent communication
//! engine.create_channel("claude-hermes", "Claude ↔ Hermes coordination")?;
//!
//! // Publish an event with a magellan trace
//! engine.publish("claude-hermes", "claude", EventPayload {
//!     status: AgentStatus::Done,
//!     working_on: "fixed path normalization bug".into(),
//!     waiting_for: None,
//!     can_start: Some("hermes can verify the fix".into()),
//!     verified: true,
//!     magellan_trace: None,
//!     extra: serde_json::Value::Null,
//! })?;
//!
//! // Another agent catches up
//! engine.subscribe("hermes", "claude-hermes")?;
//! let new_events = engine.catch_up("hermes", "claude-hermes")?;
//! ```

pub mod engine;
pub mod error;
pub mod message;
pub mod types;

pub use engine::Engine;
pub use error::EnvoyError;
pub use types::{
    AgentStatus, Channel, Event, EventPayload, MagellanDbState, MagellanTrace, Subscription,
};
