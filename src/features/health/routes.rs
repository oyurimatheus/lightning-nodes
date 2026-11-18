use axum::{Json, Router, http::StatusCode, routing::get};
use serde_json::json;
use sqlx::PgPool;

use crate::app::state::AppState;

async fn live() -> StatusCode {
    StatusCode::OK
}

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

pub type ReadinessPool = PgPool;
