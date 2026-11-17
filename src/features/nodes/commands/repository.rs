use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::features::nodes::domain::LightningNode;
use crate::shared::error::AppError;

/// Write path for the snapshot lifecycle. Implementations must guarantee:
/// - `create_loading_snapshot` starts an unfinished, non-active snapshot;
/// - `write_nodes` performs batched parameterized inserts;
/// - `publish` atomically verifies, marks `ready` with a PostgreSQL-assigned
///   `saved_at`, and updates the `active_snapshot` pointer in one transaction;
/// - `mark_failed` never touches the active snapshot.
#[async_trait]
pub trait SnapshotRepository: Send + Sync {
    async fn create_loading_snapshot(&self) -> Result<Uuid, AppError>;

    async fn write_nodes(&self, snapshot_id: Uuid, nodes: &[LightningNode])
    -> Result<(), AppError>;

    /// Returns the `saved_at` assigned by PostgreSQL.
    async fn publish(
        &self,
        snapshot_id: Uuid,
        expected_node_count: usize,
    ) -> Result<DateTime<Utc>, AppError>;

    async fn mark_failed(&self, snapshot_id: Uuid, message: &str) -> Result<(), AppError>;

    async fn active_snapshot_id(&self) -> Result<Option<Uuid>, AppError>;

    /// Remove expired inactive snapshots and their nodes. Never removes the
    /// active snapshot and always keeps at least `minimum_successful`
    /// successful snapshots (counting the active one). Returns the number of
    /// removed snapshots.
    async fn cleanup_history(
        &self,
        retention_days: u32,
        minimum_successful: u32,
    ) -> Result<u64, AppError>;
}
