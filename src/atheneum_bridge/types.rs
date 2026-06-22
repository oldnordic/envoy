use serde::{Deserialize, Serialize};

use crate::atheneum_bridge::utils::{
    default_event_limit, default_search_k, default_tool, default_trigger,
};

// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct StoreDiscoveryRequest {
    pub agent: String,
    pub discovery_type: String,
    pub target: String,
    #[serde(default)]
    pub project_id: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct StoreDiscoveryResponse {
    pub discovery_id: i64,
    pub agent: String,
    pub target: String,
    pub discovery_type: String,
}

#[derive(Debug, Deserialize)]
pub struct DiscoveriesQuery {
    pub target: String,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiscoveriesResponse {
    pub target: String,
    pub discovery_count: usize,
    pub discoveries: Vec<DiscoveryData>,
}

#[derive(Debug, Serialize)]
pub struct DiscoveryData {
    pub id: i64,
    pub name: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct StoreHandoffRequest {
    pub from_agent: String,
    pub to_agent: String,
    #[serde(default)]
    pub project_id: Option<String>,
    pub manifest: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct StoreHandoffResponse {
    pub handoff_id: i64,
    pub from_agent: String,
    pub to_agent: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct PendingHandoffQuery {
    pub agent: String,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PendingHandoffResponse {
    pub handoff: Option<HandoffData>,
}

#[derive(Debug, Deserialize)]
pub struct RecentHandoffsQuery {
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default = "default_recent_limit")]
    pub limit: i64,
}

fn default_recent_limit() -> i64 {
    10
}

#[derive(Debug, Serialize)]
pub struct HandoffData {
    pub id: i64,
    pub name: String,
    pub from_agent: String,
    pub to_agent: String,
    pub manifest: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ClaimHandoffResponse {
    pub claimed: bool,
    pub handoff_id: i64,
}

#[derive(Debug, Serialize)]
pub struct RecentHandoffsResponse {
    pub count: usize,
    pub handoffs: Vec<HandoffData>,
}

#[derive(Debug, Deserialize)]
pub struct KnowledgeQuery {
    pub target: String,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectContextQuery {
    pub project: String,
    #[serde(default = "default_context_limit")]
    pub limit: i64,
}

fn default_context_limit() -> i64 {
    8
}

#[derive(Debug, Serialize)]
pub struct ProjectContextItem {
    pub discovery_type: String,
    pub target: String,
    pub why: String,
    pub agent: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectContextResponse {
    pub project: String,
    pub items: Vec<ProjectContextItem>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_search_k")]
    pub k: usize,
    #[serde(default)]
    pub project: Option<String>,
}

// fn default_search_k — moved to utils.rs, imported via `use crate::atheneum_bridge::utils::default_search_k` above
// Keep this comment to prevent accidental re-addition.

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub project: Option<String>,
    pub count: usize,
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub score: f32,
    pub data: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct KnowledgeResponse {
    pub target: String,
    pub queried_at: String,
    pub total_entities: i64,
    pub discovery_count: usize,
    pub discoveries: Vec<DiscoveryData>,
    pub handoff_count: usize,
    pub handoffs: Vec<HandoffData>,
    pub token_savings: TokenSavings,
}

#[derive(Debug, Serialize)]
pub struct TokenSavings {
    pub unique_agents: i64,
    pub estimated_file_tokens: i64,
    pub without_sharing: i64,
    pub with_sharing: i64,
    pub saved: i64,
    pub percentage_reduction: f64,
}

#[derive(Debug, Deserialize)]
pub struct ImportMagellanSymbolRequest {
    pub magellan_db_path: String,
    pub symbol_name: String,
    pub agent_name: String,
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportMagellanBulkRequest {
    pub magellan_db_path: String,
    pub agent_name: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ImportMagellanSymbolResponse {
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ImportMagellanBulkResponse {
    pub imported_count: i64,
}

// ============================================================================
// HTTP Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TaskCreatedResponse {
    pub task_id: i64,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListTasksResponse {
    pub tasks: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskStatusRequest {
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct TaskDetailResponse {
    pub task: serde_json::Value,
    pub requirements: Vec<serde_json::Value>,
    pub blockers: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequirementRequest {
    pub statement: String,
    #[serde(default)]
    pub verification_method: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBlockerRequest {
    pub description: String,
    pub blocker_type: String,
}

#[derive(Debug, Deserialize)]
pub struct IngestJournalRequest {
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IngestJournalResponse {
    pub section_ids: Vec<i64>,
    pub applied_kanban_updates: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ToolCallInput {
    pub tool_name: String,
    pub args: serde_json::Value,
    #[serde(default)]
    pub modified_targets: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateActionRequest {
    pub agent: String,
    pub thought: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallInput>,
}

#[derive(Debug, Serialize)]
pub struct ActionTraceResponse {
    pub agent_id: i64,
    pub reasoning_log_id: i64,
    pub tool_call_ids: Vec<i64>,
    pub modified_edge_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct GetActionsQuery {
    pub agent: String,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GetActionsResponse {
    pub actions: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateClassRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClassCreatedResponse {
    pub class_id: i64,
}

#[derive(Debug, Serialize)]
pub struct ListClassesResponse {
    pub classes: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePropertyRequest {
    pub name: String,
    pub domain_class: String,
    pub range_class: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PropertyCreatedResponse {
    pub property_id: i64,
}

#[derive(Debug, Serialize)]
pub struct ListPropertiesResponse {
    pub properties: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ValidateEdgeQuery {
    pub from: String,
    pub to: String,
    pub edge: String,
}

#[derive(Debug, Serialize)]
pub struct ValidateEdgeResponse {
    pub allowed: bool,
}

#[derive(Debug, Serialize)]
pub struct SeedResponse {
    pub seeded: i64,
}

#[derive(Debug, Deserialize)]
pub struct RecordSessionRequest {
    pub session_id: String,
    pub agent: String,
    pub project: String,
    #[serde(default = "default_tool")]
    pub tool: String,
    #[serde(default = "default_trigger")]
    pub trigger: String,
    pub model: Option<String>,
    pub git_branch: Option<String>,
    pub git_head: Option<String>,
    pub parent_session_id: Option<String>,
}

// default_search_k, default_tool, default_trigger, default_event_limit moved to utils.rs

#[derive(Debug, Serialize)]
pub struct RecordSessionResponse {
    pub session_id: String,
    pub recorded: bool,
}

#[derive(Debug, Deserialize)]
pub struct QuerySessionsQuery {
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default = "default_sessions_last")]
    pub last: i64,
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionInspectQuery {
    #[serde(default = "default_event_limit_i64")]
    pub limit: i64,
}

fn default_event_limit_i64() -> i64 {
    20
}

fn default_sessions_last() -> i64 {
    5
}

#[derive(Debug, Deserialize)]
pub struct SubagentHandoverRequest {
    pub summary: String,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default = "default_outcome")]
    pub outcome: String,
}

fn default_outcome() -> String {
    "complete".to_string()
}

#[derive(Debug, Deserialize)]
pub struct EndSessionRequest {
    pub exit_status: String,
    #[serde(default)]
    pub prompt_count: u32,
    #[serde(default)]
    pub tool_call_count: u32,
    #[serde(default)]
    pub file_write_count: u32,
    #[serde(default)]
    pub commit_count: u32,
    #[serde(default)]
    pub test_run_count: u32,
    #[serde(default)]
    pub total_input_tokens: u64,
    #[serde(default)]
    pub total_output_tokens: u64,
    #[serde(default)]
    pub total_cost_usd: f64,
}

#[derive(Debug, Deserialize)]
pub struct RecordPromptRequest {
    pub session_id: String,
    pub role: String,
    #[serde(default)]
    pub sequence: u32,
    pub input_hash: String,
    pub input_tokens: Option<u64>,
    pub output_hash: Option<String>,
    pub output_tokens: Option<u64>,
    pub latency_ms: Option<u64>,
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct RecordToolCallRequest {
    pub session_id: String,
    pub tool_name: String,
    pub tool_version: Option<String>,
    pub input_hash: Option<String>,
    pub input_summary: Option<String>,
    pub output_hash: Option<String>,
    pub output_summary: Option<String>,
    pub exit_status: String,
    #[serde(default)]
    pub latency_ms: u64,
    pub input_tokens_est: Option<u64>,
    #[serde(default)]
    pub tool_category: String,
}

#[derive(Debug, Deserialize)]
pub struct RecordFileWriteRequest {
    pub session_id: String,
    pub file_path: String,
    pub file_id: Option<String>,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    #[serde(default)]
    pub lines_added: u32,
    #[serde(default)]
    pub lines_deleted: u32,
    #[serde(default)]
    pub lines_changed: u32,
    #[serde(default)]
    pub write_type: String,
}

#[derive(Debug, Deserialize)]
pub struct RecordCommitRequest {
    pub session_id: String,
    pub commit_sha: String,
    pub parent_sha: Option<String>,
    pub message: String,
    pub author: String,
    #[serde(default)]
    pub files_changed: u32,
    #[serde(default)]
    pub lines_inserted: u32,
    #[serde(default)]
    pub lines_deleted: u32,
    pub commit_type: String,
    pub feature_tag: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RecordTestRunRequest {
    pub session_id: String,
    pub test_name: String,
    pub test_suite: Option<String>,
    pub test_command: Option<String>,
    pub result: String,
    #[serde(default)]
    pub duration_ms: u64,
    pub logs_summary: Option<String>,
    pub commit_sha: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RecordFixChainRequest {
    pub session_id: String,
    pub bug_commit_sha: String,
    pub fix_commit_sha: String,
    pub fix_type: String,
    pub severity: String,
    #[serde(default)]
    pub cycles_to_fix: u32,
    #[serde(default)]
    pub time_to_fix_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct RecordBenchRunRequest {
    pub session_id: String,
    pub bench_name: String,
    pub mean_ns: Option<i64>,
    pub median_ns: Option<i64>,
    pub p95_ns: Option<i64>,
    #[serde(default)]
    pub is_regression: bool,
}

#[derive(Debug, Deserialize)]
pub struct QueryEventsQuery {
    pub session_id: Option<String>,
    pub event_type: Option<String>,
    #[serde(default = "default_event_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct RecentToolCallsQuery {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default = "default_event_limit")]
    pub limit: usize,
}

// default_search_k, default_tool, default_trigger, default_event_limit moved to utils.rs

#[derive(Debug, Deserialize)]
pub struct RecordEventRequest {
    pub session_id: String,
    pub event_type: String,
    pub entity_id: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct QueryEventsResponse {
    pub events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SessionInspectResponse {
    pub session: Option<atheneum::graph::SessionSummary>,
    pub event_count: usize,
    pub tool_call_count: usize,
    pub tool_calls: Vec<serde_json::Value>,
    pub events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ToolUsageItem {
    pub tool_name: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct RecentToolCallsResponse {
    pub count: usize,
    pub usage: Vec<ToolUsageItem>,
    pub events: Vec<serde_json::Value>,
}

// ============================================================================
// Graph Navigation Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct GraphEntityResponse {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub file_path: Option<String>,
    pub data: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct GraphEdgeResponse {
    pub id: i64,
    pub from_id: i64,
    pub to_id: i64,
    pub edge_type: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct NeighborsResponse {
    pub entity_id: i64,
    pub outgoing: Vec<GraphEdgeResponse>,
    pub incoming: Vec<GraphEdgeResponse>,
}

#[derive(Debug, Serialize)]
pub struct SubgraphViewResponse {
    pub entry: GraphEntityResponse,
    pub depth: u32,
    pub entities: Vec<GraphEntityResponse>,
    pub edges: Vec<GraphEdgeResponse>,
}

#[derive(Debug, Serialize)]
pub struct NavigateResponse {
    pub query: String,
    pub subgraphs: Vec<SubgraphViewResponse>,
}

#[derive(Debug, Serialize)]
pub struct GraphStatsResponse {
    pub total_entities: i64,
    pub total_edges: i64,
    pub entity_counts: Vec<(String, i64)>,
    pub edge_counts: Vec<(String, i64)>,
}

#[derive(Debug, Deserialize)]
pub struct NavigateQuery {
    pub q: String,
    #[serde(default = "default_search_k")]
    pub k: usize,
    #[serde(default = "default_navigate_depth")]
    pub depth: u32,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CrossSearchQuery {
    pub q: String,
    #[serde(default = "default_search_k")]
    pub k: usize,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CrossSearchResponse {
    pub query: String,
    pub language: Option<String>,
    pub count: usize,
    pub results: Vec<CrossSearchResultItem>,
}

#[derive(Debug, Serialize)]
pub struct CrossSearchResultItem {
    pub project: String,
    pub id: i64,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct CrossNavigateQuery {
    pub q: String,
    #[serde(default = "default_search_k")]
    pub k: usize,
    #[serde(default = "default_navigate_depth")]
    pub depth: u32,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CrossNavigateResponse {
    pub query: String,
    pub language: Option<String>,
    pub count: usize,
    pub views: Vec<CrossSubgraphView>,
}

#[derive(Debug, Serialize)]
pub struct CrossSubgraphView {
    pub project: String,
    pub entry_id: i64,
    pub entities: Vec<CrossSearchResultItem>,
    pub edges: Vec<CrossEdgeItem>,
}

#[derive(Debug, Serialize)]
pub struct CrossEdgeItem {
    pub id: i64,
    pub kind: String,
    pub from_id: i64,
    pub to_id: i64,
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct NeighborsQuery {
    pub depth: Option<u32>,
}

fn default_navigate_depth() -> u32 {
    2
}
