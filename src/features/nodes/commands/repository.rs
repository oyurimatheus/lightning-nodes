use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::features::nodes::domain::LightningNode;
use crate::shared::error::AppError;

#[async_trait]
pub trait SnapshotRepository: Send + Sync {
    async fn create_loading_snapshot(&self) -> Result<Uuid, AppError>;

    async fn write_nodes(&self, snapshot_id: Uuid, nodes: &[LightningNode])
    -> Result<(), AppError>;

    async fn publish(
        &self,
        snapshot_id: Uuid,
        expected_node_count: usize,
    ) -> Result<DateTime<Utc>, AppError>;

    async fn mark_failed(&self, snapshot_id: Uuid, message: &str) -> Result<(), AppError>;

    async fn active_snapshot_id(&self) -> Result<Option<Uuid>, AppError>;

    async fn cleanup_history(
        &self,
        retention_days: u32,
        minimum_successful: u32,
    ) -> Result<u64, AppError>;
}
