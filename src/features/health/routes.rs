use axum::{Json, Router, http::StatusCode, routing::get};
use serde_json::json;
use sqlx::PgPool;

use crate::app::state::AppState;

/// Liveness: the process is running.
async fn live() -> StatusCode {
    StatusCode::OK
}

/// Readiness: application and database (read pool) are reachable. A failed
/// synchronization does NOT fail readiness while the last valid snapshot
/// remains servable; staleness is exposed via `/sync/status`.
async fn ready(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "unavailable" })),
        ),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
}

/// Helper so tests / other features can reuse the readiness probe type.
pub type ReadinessPool = PgPool;
