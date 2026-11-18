use axum::Router;

use crate::app::state::AppState;

pub fn build(state: AppState) -> Router {
    crate::features::nodes::http::handlers::router(state)
}
