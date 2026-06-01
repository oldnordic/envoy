use crate::error::{EnvoyError, Result};

pub(crate) fn parse_status(s: &str) -> Result<atheneum::graph::KanbanStatus> {
    match s.to_ascii_uppercase().as_str() {
        "TODO" => Ok(atheneum::graph::KanbanStatus::Todo),
        "IN_PROGRESS" | "INPROGRESS" => Ok(atheneum::graph::KanbanStatus::InProgress),
        "DONE" => Ok(atheneum::graph::KanbanStatus::Done),
        "BLOCKED" => Ok(atheneum::graph::KanbanStatus::Blocked),
        other => Err(EnvoyError::Atheneum(anyhow::anyhow!(
            "Unknown KanbanStatus '{}'",
            other
        ))),
    }
}

pub(crate) fn parse_blocker_type(s: &str) -> Result<atheneum::graph::BlockerType> {
    match s.to_ascii_uppercase().as_str() {
        "DEPENDENCY" => Ok(atheneum::graph::BlockerType::Dependency),
        "BUG" => Ok(atheneum::graph::BlockerType::Bug),
        "INFO_GAP" | "INFOGAP" => Ok(atheneum::graph::BlockerType::InfoGap),
        other => Err(EnvoyError::Atheneum(anyhow::anyhow!(
            "Unknown BlockerType '{}'",
            other
        ))),
    }
}

pub(crate) fn entity_to_json(entity: atheneum::GraphEntity) -> serde_json::Value {
    serde_json::json!({
        "id": entity.id,
        "kind": entity.kind,
        "name": entity.name,
        "file_path": entity.file_path,
        "data": entity.data,
    })
}

pub(crate) fn default_search_k() -> usize {
    5
}

pub(crate) fn default_tool() -> String {
    "unknown".to_string()
}

pub(crate) fn default_trigger() -> String {
    "cli".to_string()
}

pub(crate) fn default_event_limit() -> usize {
    100
}
