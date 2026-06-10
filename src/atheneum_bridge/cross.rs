//! Cross-project bridge handlers — meta.db routing + lazy ATTACH.

use axum::extract::{Query, State};
use axum::Json;
use std::sync::Arc;

use crate::atheneum_bridge::types::{
    CrossEdgeItem, CrossNavigateQuery, CrossNavigateResponse, CrossSearchQuery,
    CrossSearchResponse, CrossSearchResultItem, CrossSubgraphView,
};
use crate::error::Result;
use crate::http::AppState;

fn to_result_item(hit: &atheneum::CrossSearchResult) -> CrossSearchResultItem {
    CrossSearchResultItem {
        project: hit.project.clone(),
        id: hit.id,
        kind: hit.kind.clone(),
        name: hit.name.clone(),
        file_path: hit.file_path.clone(),
        data: hit.data.clone(),
    }
}

fn to_edge_item(edge: &atheneum::CrossEdge) -> CrossEdgeItem {
    CrossEdgeItem {
        id: edge.id,
        kind: edge.kind.clone(),
        from_id: edge.from_id,
        to_id: edge.to_id,
        data: edge.data.clone(),
    }
}

/// GET /atheneum/cross/search?q=...&language=...&k=N
pub async fn get_cross_search(
    State(_state): State<Arc<AppState>>,
    Query(query): Query<CrossSearchQuery>,
) -> Result<Json<CrossSearchResponse>> {
    let q = query.q.clone();
    let k = query.k.max(1);
    let language = query.language.clone();
    let hits = tokio::task::spawn_blocking(move || {
        let mut router = atheneum::CrossRouter::open()?;
        router
            .cross_search(&q, language.as_deref(), k)
            .map_err(crate::error::EnvoyError::from)
    })
    .await
    .map_err(|_| crate::error::EnvoyError::InvalidEntity("blocking task panicked".into()))??;

    Ok(Json(CrossSearchResponse {
        query: query.q,
        language: query.language,
        count: hits.len(),
        results: hits.iter().map(to_result_item).collect(),
    }))
}

/// GET /atheneum/cross/navigate?q=...&language=...&k=N&depth=D
pub async fn get_cross_navigate(
    State(_state): State<Arc<AppState>>,
    Query(query): Query<CrossNavigateQuery>,
) -> Result<Json<CrossNavigateResponse>> {
    let q = query.q.clone();
    let k = query.k.max(1);
    let depth = query.depth.max(1);
    let language = query.language.clone();
    let views = tokio::task::spawn_blocking(move || {
        let mut router = atheneum::CrossRouter::open()?;
        router
            .cross_navigate(&q, language.as_deref(), k, depth)
            .map_err(crate::error::EnvoyError::from)
    })
    .await
    .map_err(|_| crate::error::EnvoyError::InvalidEntity("blocking task panicked".into()))??;

    Ok(Json(CrossNavigateResponse {
        query: query.q,
        language: query.language,
        count: views.len(),
        views: views
            .iter()
            .map(|v| CrossSubgraphView {
                project: v.project.clone(),
                entry_id: v.entry_id,
                entities: v.entities.iter().map(to_result_item).collect(),
                edges: v.edges.iter().map(to_edge_item).collect(),
            })
            .collect(),
    }))
}
