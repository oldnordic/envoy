// Envoy → Atheneum Backfill Tool
//
// Reads messages from envoy's SQLite database and transforms them into
// Atheneum discoveries for semantic search and knowledge persistence.

#[cfg(feature = "atheneum")]
use std::collections::HashMap;

use anyhow::Result;

// Atheneum integration (requires atheneum feature)
#[cfg(feature = "atheneum")]
use std::path::Path;

#[cfg(feature = "atheneum")]
use sqlitegraph::GraphEntity;

#[cfg(feature = "atheneum")]
use sqlitegraph::SqliteGraph;

#[cfg(feature = "atheneum")]
use atheneum::AtheneumGraph;

#[cfg(feature = "atheneum")]
use serde_json::Value;

#[cfg(feature = "atheneum")]
const KIND_AGENT: &str = "EnvoyAgent";

#[cfg(feature = "atheneum")]
const KIND_MESSAGE: &str = "EnvoyMessage";

#[cfg(feature = "atheneum")]
struct AgentInfo {
    agent_id: String,
    display_name: String,
    kind: String,
}

#[cfg(feature = "atheneum")]
struct BackfillStats {
    messages_processed: usize,
    direct_processed: usize,
    system_processed: usize,
    handoffs_processed: usize,
    errors: usize,
    discoveries_created: usize,
    handoffs_created: usize,
}

#[cfg(feature = "atheneum")]
impl BackfillStats {
    fn new() -> Self {
        Self {
            messages_processed: 0,
            direct_processed: 0,
            system_processed: 0,
            handoffs_processed: 0,
            errors: 0,
            discoveries_created: 0,
            handoffs_created: 0,
        }
    }

    fn print_summary(&self) {
        println!("\n=== Backfill Summary ===");
        println!("Messages processed: {}", self.messages_processed);
        println!("  Direct: {}", self.direct_processed);
        println!("  System: {}", self.system_processed);
        println!("  Handoff: {}", self.handoffs_processed);
        println!("Discoveries created: {}", self.discoveries_created);
        println!("Handoffs created: {}", self.handoffs_created);
        println!("Errors: {}", self.errors);
        println!("========================\n");
    }
}

#[cfg(feature = "atheneum")]
fn load_agents(graph: &SqliteGraph) -> Result<HashMap<String, AgentInfo>> {
    let entities = graph.find_entities_by_kind(KIND_AGENT)?;
    let mut agents = HashMap::new();

    for entity in entities {
        let display_name = entity
            .data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let kind = entity
            .data
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("worker")
            .to_string();

        let info = AgentInfo {
            agent_id: entity.name.clone(),
            display_name,
            kind,
        };

        agents.insert(entity.name.clone(), info);
    }

    Ok(agents)
}

#[cfg(feature = "atheneum")]
fn resolve_agent_name(agent_id: &str, agents: &HashMap<String, AgentInfo>) -> String {
    agents
        .get(agent_id)
        .map(|a| a.display_name.clone())
        .unwrap_or_else(|| agent_id.to_string())
}

#[cfg(feature = "atheneum")]
fn classify_discovery_type(context_id: &str, _content: &str) -> &'static str {
    let context_lower = context_id.to_lowercase();

    if context_lower.contains("review") || context_lower.contains("pr") {
        return "code_review";
    }
    if context_lower.contains("push") || context_lower.contains("commit") {
        return "git_operation";
    }
    if context_lower.contains("doc") {
        return "documentation";
    }
    if context_lower.starts_with("re:") {
        return "response";
    }

    "coordination"
}

#[cfg(feature = "atheneum")]
fn get_target(context_id: Option<&str>) -> String {
    context_id
        .filter(|s| !s.is_empty())
        .unwrap_or("agent-coordination")
        .to_string()
}

#[cfg(feature = "atheneum")]
fn truncate_text(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...[truncated]", &s[..max_len])
    } else {
        s.to_string()
    }
}

#[cfg(feature = "atheneum")]
fn process_direct_message(
    msg: &GraphEntity,
    agents: &HashMap<String, AgentInfo>,
    atheneum: &AtheneumGraph,
    stats: &mut BackfillStats,
) -> Result<()> {
    let _msg_type = msg
        .data
        .get("msg_type")
        .and_then(|v| v.as_str())
        .unwrap_or("direct");
    let from_agent = msg.data.get("from").and_then(|v| v.as_str()).unwrap_or("");
    let to_agent = msg.data.get("to").and_then(|v| v.as_str()).unwrap_or("");
    let context_id = msg.data.get("context_id").and_then(|v| v.as_str());
    let timestamp = msg
        .data
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let sequence_id = msg
        .data
        .get("sequence_id")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let content = if let Some(parts) = msg.data.get("parts").and_then(|v| v.as_array()) {
        parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };

    let from_name = resolve_agent_name(from_agent, agents);
    let to_name = resolve_agent_name(to_agent, agents);
    let target = get_target(context_id);
    let discovery_type =
        classify_discovery_type(context_id.unwrap_or(&target.to_string()), &content);

    let mut metadata = serde_json::json!({
        "content": truncate_text(&content, 10000),
        "timestamp": timestamp,
        "original_msg_id": msg.name,
        "sequence_id": sequence_id,
        "from_agent_id": from_agent,
        "to_agent": to_name,
        "truncated": content.len() > 10000
    });

    if let Some(ctx) = context_id {
        metadata["context_id"] = Value::String(ctx.to_string());
    }

    match atheneum.store_discovery(&from_name, discovery_type, &target, metadata) {
        Ok(_) => {
            stats.discoveries_created += 1;
            stats.direct_processed += 1;
        }
        Err(e) => {
            eprintln!("Error storing discovery for {}: {}", msg.name, e);
            stats.errors += 1;
        }
    }

    Ok(())
}

#[cfg(feature = "atheneum")]
fn process_system_message(
    msg: &GraphEntity,
    agents: &HashMap<String, AgentInfo>,
    atheneum: &AtheneumGraph,
    stats: &mut BackfillStats,
) -> Result<()> {
    let from_agent = msg
        .data
        .get("from")
        .and_then(|v| v.as_str())
        .unwrap_or("envoy");
    let to_agent = msg.data.get("to").and_then(|v| v.as_str()).unwrap_or("");
    let context_id = msg.data.get("context_id").and_then(|v| v.as_str());
    let timestamp = msg
        .data
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let text = if let Some(parts) = msg.data.get("parts").and_then(|v| v.as_array()) {
        parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };

    let event_data: Value = match serde_json::from_str(&text) {
        Ok(data) => data,
        Err(_) => {
            eprintln!(
                "Warning: Could not parse JSON from system message {}",
                msg.name
            );
            serde_json::json!({
                "raw_content": truncate_text(&text, 10000),
                "parse_error": true
            })
        }
    };

    let discovery_type = event_data
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("system_event");

    let target = event_data
        .get("hook_name")
        .and_then(|v| v.as_str())
        .or_else(|| event_data.get("project").and_then(|v| v.as_str()))
        .unwrap_or(context_id.unwrap_or("system"));

    let to_name = resolve_agent_name(to_agent, agents);

    let mut metadata = event_data.clone();
    metadata["timestamp"] = Value::String(timestamp.to_string());
    metadata["original_msg_id"] = Value::String(msg.name.clone());
    metadata["to_agent"] = Value::String(to_name);

    match atheneum.store_discovery(from_agent, discovery_type, target, metadata) {
        Ok(_) => {
            stats.discoveries_created += 1;
            stats.system_processed += 1;
        }
        Err(e) => {
            eprintln!("Error storing discovery for {}: {}", msg.name, e);
            stats.errors += 1;
        }
    }

    Ok(())
}

#[cfg(feature = "atheneum")]
fn backfill(envoy_db_path: &str, atheneum_db_path: &str) -> Result<BackfillStats> {
    println!("Opening envoy database: {}", envoy_db_path);
    let envoy_graph = SqliteGraph::open(Path::new(envoy_db_path))?;

    println!("Opening atheneum database: {}", atheneum_db_path);
    let atheneum = AtheneumGraph::open(Path::new(atheneum_db_path))?;

    println!("Loading agents from envoy...");
    let agents = load_agents(&envoy_graph)?;
    println!("Loaded {} agents", agents.len());

    for (id, info) in agents.iter().take(5) {
        println!("  {} -> {}", id, info.display_name);
    }
    if agents.len() > 5 {
        println!("  ... and {} more", agents.len() - 5);
    }

    println!("Loading messages from envoy...");
    let messages = envoy_graph.find_entities_by_kind(KIND_MESSAGE)?;
    println!("Loaded {} messages", messages.len());

    let mut stats = BackfillStats::new();

    for msg in &messages {
        stats.messages_processed += 1;

        let msg_type = msg
            .data
            .get("msg_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        match msg_type {
            "direct" => {
                if let Err(e) = process_direct_message(msg, &agents, &atheneum, &mut stats) {
                    eprintln!("Error processing direct message {}: {}", msg.name, e);
                    stats.errors += 1;
                }
            }
            "system" => {
                if let Err(e) = process_system_message(msg, &agents, &atheneum, &mut stats) {
                    eprintln!("Error processing system message {}: {}", msg.name, e);
                    stats.errors += 1;
                }
            }
            "handoff" => {
                stats.handoffs_processed += 1;
            }
            _ => {
                eprintln!("Unknown message type: {} for {}", msg_type, msg.name);
                stats.errors += 1;
            }
        }

        if stats.messages_processed.is_multiple_of(50) {
            println!(
                "Processed {}/{} messages",
                stats.messages_processed,
                messages.len()
            );
        }
    }

    Ok(stats)
}

#[cfg(not(feature = "atheneum"))]
fn backfill(_envoy_db_path: &str, _atheneum_db_path: &str) -> Result<()> {
    eprintln!("Error: backfill requires the 'atheneum' feature.");
    eprintln!("Compile with: cargo build --release --bin backfill --features atheneum");
    std::process::exit(1);
}

#[cfg(feature = "atheneum")]
fn run() -> Result<()> {
    println!("Envoy → Atheneum Backfill Tool v0.1.0");
    println!("===================================\n");

    let args: Vec<String> = std::env::args().collect();

    let envoy_db = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("envoy.db");

    let atheneum_db = args
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or("atheneum.db");

    println!("Configuration:");
    println!("  Envoy DB: {}", envoy_db);
    println!("  Atheneum DB: {}", atheneum_db);
    println!();

    let stats = backfill(envoy_db, atheneum_db)?;
    stats.print_summary();
    Ok(())
}

#[cfg(not(feature = "atheneum"))]
fn run() -> Result<()> {
    backfill("", "")
}

fn main() -> Result<()> {
    run()
}
