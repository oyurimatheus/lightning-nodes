/// Shared application state handed to every handler.
pub struct AppState {
    pub reader: std::sync::Arc<dyn crate::features::nodes::queries::repository::NodeReader>,
    pub repository:
        std::sync::Arc<dyn crate::features::nodes::commands::repository::SnapshotRepository>,
    pub status: std::sync::Arc<crate::features::synchronization::status::SyncStatusHandle>,
    pub pool: sqlx::PgPool,
    pub include_saved_at: bool,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
            repository: self.repository.clone(),
            status: self.status.clone(),
            pool: self.pool.clone(),
            include_saved_at: self.include_saved_at,
        }
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("include_saved_at", &self.include_saved_at)
            .finish()
    }
}
