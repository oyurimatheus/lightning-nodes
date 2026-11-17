use std::sync::Arc;

use crate::features::nodes::commands::repository::SnapshotRepository;
use crate::shared::error::AppError;

/// Retention policy execution. Deletion happens in a single bounded SQL
/// statement (see `SnapshotRepository::cleanup_history`), separate from HTTP
/// request processing, and it can never remove the active snapshot or drop
/// history below the configured minimum.
pub struct HistoryCleanup {
    repository: Arc<dyn SnapshotRepository>,
}

impl HistoryCleanup {
    pub fn new(repository: Arc<dyn SnapshotRepository>) -> Self {
        Self { repository }
    }

    /// Returns the number of removed snapshots.
    pub async fn run(
        &self,
        retention_days: u32,
        minimum_successful_snapshots: u32,
    ) -> Result<u64, AppError> {
        self.repository
            .cleanup_history(retention_days, minimum_successful_snapshots)
            .await
    }
}
