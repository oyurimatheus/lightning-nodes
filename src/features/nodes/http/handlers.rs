use axum::Json;
use axum::extract::State;
use serde_json::json;
use uuid::Uuid;

use crate::app::state::AppState;
use crate::features::health;
use crate::shared::error::ApiError;

use super::dto::NodeResponse;

/// `GET /nodes`: nodes of the latest successfully published snapshot. Reads
/// only from PostgreSQL; the external provider is never called here.
pub async fn list_nodes(
    State(state): State<AppState>,
) -> Result<Json<Vec<NodeResponse>>, ApiError> {
    let nodes = state.reader.list_active_nodes().await.map_err(ApiError)?;
    let mut body = Vec::with_capacity(nodes.len());
    for node in &nodes {
        body.push(NodeResponse::from_active(node, state.include_saved_at).map_err(ApiError)?);
    }
    Ok(Json(body))
}

/// `GET /sync/status`: synchronization state, last success, last failure, last
/// payload size, and the currently active snapshot identifier.
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

/// Build the HTTP router for the nodes feature, plus health endpoints.
pub fn router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route("/nodes", axum::routing::get(list_nodes))
        .route("/sync/status", axum::routing::get(sync_status))
        .merge(health::routes::routes())
        .with_state(state)
}
