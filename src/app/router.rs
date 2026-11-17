use axum::Router;

use crate::app::state::AppState;

/// Build the full application router: nodes endpoints, synchronization
/// status, and health probes.
pub fn build(state: AppState) -> Router {
    crate::features::nodes::http::handlers::router(state)
}
