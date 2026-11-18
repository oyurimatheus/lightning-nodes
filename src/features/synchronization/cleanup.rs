use std::sync::Arc;

use crate::features::nodes::commands::repository::SnapshotRepository;
use crate::shared::error::AppError;

pub struct HistoryCleanup {
    repository: Arc<dyn SnapshotRepository>,
}

impl HistoryCleanup {
    pub fn new(repository: Arc<dyn SnapshotRepository>) -> Self {
        Self { repository }
    }

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
