use axum::extract::State;
use axum::Json;
use std::sync::Arc;

use crate::atheneum_bridge::types::*;
use crate::error::Result;
use crate::http::AppState;

pub async fn post_import_magellan_symbol(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ImportMagellanSymbolRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let magellan_path = std::path::PathBuf::from(req.magellan_db_path);
    let symbol_name = req.symbol_name;
    let agent_name = req.agent_name;
    let project_id = req.project_id;

    let result: Option<i64> = state
        .with_atheneum_async(move |atheneum| {
            atheneum
                .import_symbol_from_magellan(
                    &magellan_path,
                    &symbol_name,
                    &agent_name,
                    project_id.as_deref(),
                )
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;

    if let Some(discovery_id) = result {
        Ok((
            axum::http::StatusCode::CREATED,
            Json(ImportMagellanSymbolResponse {
                found: true,
                discovery_id: Some(discovery_id),
            }),
        ))
    } else {
        Ok((
            axum::http::StatusCode::OK,
            Json(ImportMagellanSymbolResponse {
                found: false,
                discovery_id: None,
            }),
        ))
    }
}

/// POST /atheneum/import-magellan/all — bulk-import every Symbol entity
/// from a magellan sqlitegraph DB into atheneum Discoveries.
pub async fn post_import_magellan_all(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ImportMagellanBulkRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let magellan_path = std::path::PathBuf::from(req.magellan_db_path);
    let agent_name = req.agent_name;
    let project_id = req.project_id;
    let limit = req.limit;

    let count: usize = state
        .with_atheneum_async(move |atheneum| {
            atheneum
                .import_all_symbols_from_magellan(
                    &magellan_path,
                    &agent_name,
                    project_id.as_deref(),
                    limit,
                )
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ImportMagellanBulkResponse {
            imported_count: count as i64,
        }),
    ))
}
