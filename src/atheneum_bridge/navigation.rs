//! Graph navigation bridge handlers — GET entity/edge/neighbors/subgraph/navigate/stats

use axum::extract::{Path, Query, State};
use axum::Json;
use std::sync::Arc;

use crate::atheneum_bridge::types::*;
use crate::error::Result;
use crate::http::AppState;

fn to_entity_resp(e: &atheneum::GraphEntity) -> GraphEntityResponse {
    GraphEntityResponse {
        id: e.id,
        kind: e.kind.clone(),
        name: e.name.clone(),
        file_path: e.file_path.clone(),
        data: e.data.clone(),
    }
}

fn to_edge_resp(edge: &atheneum::GraphEdge) -> GraphEdgeResponse {
    GraphEdgeResponse {
        id: edge.id,
        from_id: edge.from_id,
        to_id: edge.to_id,
        edge_type: edge.edge_type.clone(),
        data: edge.data.clone(),
    }
}

fn to_subgraph_resp(sg: &atheneum::graph::SubgraphView) -> SubgraphViewResponse {
    SubgraphViewResponse {
        entry: to_entity_resp(&sg.entry),
        depth: sg.depth,
        entities: sg.entities.iter().map(to_entity_resp).collect(),
        edges: sg.edges.iter().map(to_edge_resp).collect(),
    }
}

/// GET /atheneum/graph/entities/{id} — read a single entity
pub async fn get_entity(
    State(state): State<Arc<AppState>>,
    Path(entity_id): Path<i64>,
) -> Result<Json<GraphEntityResponse>> {
    let atheneum_path = state.require_atheneum_path()?;
    let entity: atheneum::GraphEntity = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.get_entity(entity_id)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(Json(to_entity_resp(&entity)))
}

/// GET /atheneum/graph/edges/{id} — read a single edge
pub async fn get_edge(
    State(state): State<Arc<AppState>>,
    Path(edge_id): Path<i64>,
) -> Result<Json<GraphEdgeResponse>> {
    let atheneum_path = state.require_atheneum_path()?;
    let edge: atheneum::GraphEdge = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.get_edge(edge_id).map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(Json(to_edge_resp(&edge)))
}

/// GET /atheneum/graph/entities/{id}/neighbors?depth=N
/// Default depth = 0 means just one-hop edges (entity data NOT included).
/// If depth > 0 returns a full subgraph of N hops.
pub async fn get_neighbors(
    State(state): State<Arc<AppState>>,
    Path(entity_id): Path<i64>,
    Query(query): Query<NeighborsQuery>,
) -> Result<Json<serde_json::Value>> {
    let atheneum_path = state.require_atheneum_path()?;
    let depth = query.depth.unwrap_or(0);

    if depth > 0 {
        let sg: atheneum::graph::SubgraphView = state
            .with_engine_async(move |_engine| {
                use atheneum::graph::AtheneumGraph;
                let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                    .map_err(crate::error::EnvoyError::from)?;
                g.get_subgraph(entity_id, depth)
                    .map_err(crate::error::EnvoyError::from)
            })
            .await?;
        let resp_value = serde_json::to_value(to_subgraph_resp(&sg))
            .map_err(crate::error::EnvoyError::Serialization)?;
        Ok(Json(resp_value))
    } else {
        let (outgoing, incoming): (Vec<_>, Vec<_>) = state
            .with_engine_async(move |_engine| {
                use atheneum::graph::AtheneumGraph;
                let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                    .map_err(crate::error::EnvoyError::from)?;
                g.get_neighbors(entity_id)
                    .map_err(crate::error::EnvoyError::from)
            })
            .await?;

        let resp = NeighborsResponse {
            entity_id,
            outgoing: outgoing.iter().map(to_edge_resp).collect(),
            incoming: incoming.iter().map(to_edge_resp).collect(),
        };
        let resp_value =
            serde_json::to_value(resp).map_err(crate::error::EnvoyError::Serialization)?;
        Ok(Json(resp_value))
    }
}

/// GET /atheneum/graph/navigate?query=X[&k=N&depth=D&project=P]
/// Semantic search → walk graph → return subgraph views.
pub async fn get_navigate(
    State(state): State<Arc<AppState>>,
    Query(query): Query<NavigateQuery>,
) -> Result<Json<NavigateResponse>> {
    let q = query.q.clone();
    let k = query.k.max(1);
    let depth = query.depth.max(1);
    let project = query.project.clone();
    let atheneum_path = state.require_atheneum_path()?;

    let subgraphs: Vec<SubgraphViewResponse> = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            let views = g
                .navigate(&q, k, depth, project.as_deref())
                .map_err(crate::error::EnvoyError::from)?;
            let resp: Vec<SubgraphViewResponse> = views.iter().map(to_subgraph_resp).collect();
            Ok::<_, crate::error::EnvoyError>(resp)
        })
        .await?;

    Ok(Json(NavigateResponse {
        query: query.q,
        subgraphs,
    }))
}

/// GET /atheneum/graph/stats — topological summary
pub async fn get_stats(State(state): State<Arc<AppState>>) -> Result<Json<GraphStatsResponse>> {
    let atheneum_path = state.require_atheneum_path()?;
    let stats: atheneum::graph::GraphStats = state
        .with_engine_async(move |_engine| {
            use atheneum::graph::AtheneumGraph;
            let g = AtheneumGraph::open(std::path::Path::new(&atheneum_path))
                .map_err(crate::error::EnvoyError::from)?;
            g.graph_stats().map_err(crate::error::EnvoyError::from)
        })
        .await?;

    Ok(Json(GraphStatsResponse {
        total_entities: stats.total_entities,
        total_edges: stats.total_edges,
        entity_counts: stats.entity_counts,
        edge_counts: stats.edge_counts,
    }))
}
