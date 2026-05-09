//! # Envoy — Message/Coordination Server for AI Coding Agents
//!
//! HTTP+JSON coordination server built on sqlitegraph.
//! Replaces file-based message passing with real-time structured messaging,
//! agent identity management, and subagent handoff protocol.

pub mod agent;
pub mod atheneum_bridge;
pub mod audit;
pub mod circuit;
pub mod dependency;
pub mod engine;
pub mod error;
pub mod event;
pub mod http;
pub mod message;
pub mod monitor;
pub mod rate_limit;
pub mod server;
pub mod status;
pub mod task;
pub mod types;

// Core types
pub use engine::Engine;
pub use types::{
    AgentStatus, Channel, EngineStats, Event, EventPayload, MagellanDbState, MagellanTrace,
    Subscription,
};

// Agent types
pub use agent::{AgentInfo, AgentRegistry};

// Message types
pub use message::{
    CompletionStatus, HandoffData, MagellanTracePayload, MessageEnvelope, MessageStore,
    MessageType, Part, PartContent, QualityGateResult, VerificationState, WhatIsStubbed,
    WhatWasDone,
};

// Rate limit types
pub use rate_limit::{
    HybridRateLimiter, HybridRateLimiterStats, RateLimitConfig, RateLimitDecision, RateLimitState,
    RateLimitStore, TokenBucket,
};

pub use error::EnvoyError;
pub use event::{EnvoyEvent, EventSeverity, EventType};
pub use http::AppState;
pub use task::{Task, TaskState};
