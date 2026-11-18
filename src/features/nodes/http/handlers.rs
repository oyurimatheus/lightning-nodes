use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use std::fmt::Display;
use uuid::Uuid;

use crate::app::state::AppState;
use crate::features::health;
use crate::shared::error::ApiError;

use super::dto::NodeResponse;

const DEFAULT_PAGE: u64 = 1;
const DEFAULT_PER_PAGE: u64 = 20;
const MAX_PER_PAGE: u64 = 100;

#[derive(Debug, Deserialize)]
pub struct NodeListQuery {
    #[serde(default = "default_page")]
    page: u64,
    #[serde(default = "default_per_page")]
    per_page: u64,
}

fn default_page() -> u64 {
    DEFAULT_PAGE
}

fn default_per_page() -> u64 {
    DEFAULT_PER_PAGE
}

pub async fn list_nodes(
    State(state): State<AppState>,
    Query(query): Query<NodeListQuery>,
) -> Result<Response, ApiError> {
    let Some(offset) = pagination_offset(query.page, query.per_page) else {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "page must be at least 1 and per_page must be between 1 and 100"
            })),
        )
            .into_response());
    };

    let page = state
        .reader
        .list_active_nodes(query.per_page as i64, offset)
        .await
        .map_err(ApiError)?;
    let mut body = Vec::with_capacity(page.nodes.len());
    for node in &page.nodes {
        body.push(NodeResponse::from_active(node, state.include_saved_at).map_err(ApiError)?);
    }

    let total_pages = page.total_count.div_ceil(query.per_page);
    let mut response = Json(body).into_response();
    let headers = response.headers_mut();
    insert_pagination_header(headers, "x-total-count", page.total_count);
    insert_pagination_header(headers, "x-page", query.page);
    insert_pagination_header(headers, "x-per-page", query.per_page);
    insert_pagination_header(headers, "x-total-pages", total_pages);
    Ok(response)
}

fn pagination_offset(page: u64, per_page: u64) -> Option<i64> {
    if page == 0 || per_page == 0 || per_page > MAX_PER_PAGE {
        return None;
    }

    let offset = page.checked_sub(1)?.checked_mul(per_page)?;
    i64::try_from(offset).ok()
}

fn insert_pagination_header(headers: &mut HeaderMap, name: &'static str, value: impl Display) {
    if let Ok(value) = HeaderValue::from_str(&value.to_string()) {
        headers.insert(HeaderName::from_static(name), value);
    }
}

pub async fn sync_status(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let status = state.status.snapshot().await;
    let active_snapshot_id: Option<Uuid> = state
        .repository
        .active_snapshot_id()
        .await
        .map_err(ApiError)?;

    Ok(Json(json!({
        "state": status.state,
        "last_success": status.last_success,
        "last_failure": status.last_failure,
        "last_snapshot_count": status.last_snapshot_count,
        "active_snapshot_id": active_snapshot_id,
    })))
}

pub fn router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route("/nodes", axum::routing::get(list_nodes))
        .route("/sync/status", axum::routing::get(sync_status))
        .merge(health::routes::routes())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_uses_a_one_based_page_and_bounded_page_size() {
        assert_eq!(pagination_offset(1, 100), Some(0));
        assert_eq!(pagination_offset(2, 25), Some(25));
        assert_eq!(pagination_offset(0, 10), None);
        assert_eq!(pagination_offset(1, 0), None);
        assert_eq!(pagination_offset(1, MAX_PER_PAGE + 1), None);
        assert_eq!(pagination_offset(u64::MAX, 100), None);
    }
}
