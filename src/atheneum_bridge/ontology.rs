use axum::extract::{Query, State};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::atheneum_bridge::types::*;
use crate::error::Result;
use crate::http::AppState;

pub async fn post_ontology_class(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateClassRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let id = state
        .with_atheneum_async(move |g| {
            g.define_class(&req.name, req.description.as_deref())
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(ClassCreatedResponse { class_id: id }),
    ))
}

/// GET /atheneum/ontology/classes — list registered classes.
pub async fn get_ontology_classes(
    State(state): State<Arc<AppState>>,
) -> Result<impl axum::response::IntoResponse> {
    let classes: Vec<serde_json::Value> = state
        .with_atheneum_async(move |g| {
            let classes = g.list_classes().map_err(crate::error::EnvoyError::from)?;
            Ok(classes
                .into_iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "name": c.name,
                        "description": c.description,
                    })
                })
                .collect())
        })
        .await?;
    Ok(Json(ListClassesResponse { classes }))
}

/// POST /atheneum/ontology/properties — register or update an ontology property.
pub async fn post_ontology_property(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreatePropertyRequest>,
) -> Result<impl axum::response::IntoResponse> {
    let id = state
        .with_atheneum_async(move |g| {
            g.define_property(
                &req.name,
                &req.domain_class,
                &req.range_class,
                req.description.as_deref(),
            )
            .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(PropertyCreatedResponse { property_id: id }),
    ))
}

/// GET /atheneum/ontology/properties — list registered properties.
pub async fn get_ontology_properties(
    State(state): State<Arc<AppState>>,
) -> Result<impl axum::response::IntoResponse> {
    let properties: Vec<serde_json::Value> = state
        .with_atheneum_async(move |g| {
            let properties = g
                .list_properties()
                .map_err(crate::error::EnvoyError::from)?;
            Ok(properties
                .into_iter()
                .map(|p| {
                    json!({
                        "id": p.id,
                        "name": p.name,
                        "domain_class": p.domain_class,
                        "range_class": p.range_class,
                        "description": p.description,
                    })
                })
                .collect())
        })
        .await?;
    Ok(Json(ListPropertiesResponse { properties }))
}

/// GET /atheneum/ontology/validate?from=&to=&edge= — open-mode validation
/// of a candidate `(from)-[edge]->(to)`.
pub async fn get_ontology_validate(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ValidateEdgeQuery>,
) -> Result<impl axum::response::IntoResponse> {
    let allowed = state
        .with_atheneum_async(move |g| {
            g.validate_edge(&query.from, &query.to, &query.edge)
                .map_err(crate::error::EnvoyError::from)
        })
        .await?;
    Ok(Json(ValidateEdgeResponse { allowed }))
}

/// POST /atheneum/ontology/seed — idempotently populate the 15 standard
/// classes (Agent, Task, Project, CodeSymbol, WikiPage, …).
pub async fn post_ontology_seed(
    State(state): State<Arc<AppState>>,
) -> Result<impl axum::response::IntoResponse> {
    let seeded: i64 = state
        .with_atheneum_async(move |g| {
            g.seed_standard_ontology()
                .map_err(crate::error::EnvoyError::from)?;
            Ok(g.list_classes()
                .map_err(crate::error::EnvoyError::from)?
                .len() as i64)
        })
        .await?;
    Ok(Json(SeedResponse { seeded }))
}
