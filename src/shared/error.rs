use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug, Clone, thiserror::Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("provider unavailable: {0}")]
    ProviderUnavailable(String),

    #[error("provider timeout")]
    ProviderTimeout,

    #[error("provider returned unexpected status {status}")]
    ProviderUnexpectedStatus { status: u16 },

    #[error("invalid provider payload: {0}")]
    InvalidProviderPayload(String),

    #[error("invalid capacity: {0}")]
    InvalidCapacity(String),

    #[error("invalid timestamp: {0}")]
    InvalidTimestamp(String),

    #[error("duplicate node public key: {0}")]
    DuplicateNode(String),

    #[error("database unavailable: {0}")]
    DatabaseUnavailable(String),

    #[error("persistence failure: {0}")]
    PersistenceFailure(String),

    #[error("snapshot publication failure: {0}")]
    SnapshotPublication(String),
}

impl AppError {
    pub fn category(&self) -> &'static str {
        match self {
            AppError::Configuration(_) => "configuration_error",
            AppError::ProviderUnavailable(_) => "provider_unavailable",
            AppError::ProviderTimeout => "provider_timeout",
            AppError::ProviderUnexpectedStatus { .. } => "provider_unexpected_status",
            AppError::InvalidProviderPayload(_) => "invalid_provider_payload",
            AppError::InvalidCapacity(_) => "invalid_capacity",
            AppError::InvalidTimestamp(_) => "invalid_timestamp",
            AppError::DuplicateNode(_) => "duplicate_node",
            AppError::DatabaseUnavailable(_) => "database_unavailable",
            AppError::PersistenceFailure(_) => "persistence_failure",
            AppError::SnapshotPublication(_) => "snapshot_publication_failure",
        }
    }

    pub fn is_provider_error(&self) -> bool {
        matches!(
            self,
            AppError::ProviderUnavailable(_)
                | AppError::ProviderTimeout
                | AppError::ProviderUnexpectedStatus { .. }
                | AppError::InvalidProviderPayload(_)
                | AppError::InvalidCapacity(_)
                | AppError::InvalidTimestamp(_)
                | AppError::DuplicateNode(_)
        )
    }
}

#[derive(Debug)]
pub struct ApiError(pub AppError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::INTERNAL_SERVER_ERROR;
        (status, Json(json!({ "error": "internal server error" }))).into_response()
    }
}
