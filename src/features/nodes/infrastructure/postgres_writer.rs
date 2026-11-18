use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder, Row, Transaction};
use std::time::Instant;
use uuid::Uuid;

use crate::app::config::MAX_INSERT_PARAMS;
use crate::features::nodes::commands::repository::SnapshotRepository;
use crate::features::nodes::domain::LightningNode;
use crate::shared::error::AppError;

const PUBLICATION_LOCK_KEY: i64 = 627_512_827_001;

pub struct PostgresSnapshotRepository {
    pool: PgPool,
    batch_size: usize,
}

impl PostgresSnapshotRepository {
    pub fn new(pool: PgPool, batch_size: usize) -> Self {
        Self { pool, batch_size }
    }

    fn effective_batch_size(&self) -> usize {
        self.batch_size.clamp(1, MAX_INSERT_PARAMS / 5)
    }
}

#[async_trait]
impl SnapshotRepository for PostgresSnapshotRepository {
    async fn create_loading_snapshot(&self) -> Result<Uuid, AppError> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO node_snapshots (id, status, created_at) VALUES ($1, 'loading', now())",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::PersistenceFailure(format!("failed to create snapshot: {e}")))?;
        tracing::debug!(snapshot_id = %id, "created loading snapshot");
        Ok(id)
    }

    async fn write_nodes(
        &self,
        snapshot_id: Uuid,
        nodes: &[LightningNode],
    ) -> Result<(), AppError> {
        let batch_size = self.effective_batch_size();
        let started = Instant::now();

        for chunk in nodes.chunks(batch_size) {
            let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO lightning_nodes (snapshot_id, public_key, alias, capacity_sats, first_seen) ",
            );
            builder.push_values(chunk.iter(), |mut row, node| {
                row.push_bind(snapshot_id)
                    .push_bind(&node.public_key)
                    .push_bind(&node.alias)
                    .push_bind(node.capacity_sats)
                    .push_bind(node.first_seen);
            });
            builder.build().execute(&self.pool).await.map_err(|e| {
                AppError::PersistenceFailure(format!(
                    "batch insert failed (batch of {} nodes): {e}",
                    chunk.len()
                ))
            })?;
        }

        tracing::debug!(
            snapshot_id = %snapshot_id,
            nodes_persisted = nodes.len(),
            duration_ms = started.elapsed().as_millis() as u64,
            "nodes written in batches"
        );
        Ok(())
    }

    async fn publish(
        &self,
        snapshot_id: Uuid,
        expected_node_count: usize,
    ) -> Result<DateTime<Utc>, AppError> {
        let started = Instant::now();
        let mut tx = self.pool.begin().await.map_err(|e| {
            AppError::SnapshotPublication(format!("failed to begin publication transaction: {e}"))
        })?;

        let result = publish_in_transaction(&mut tx, snapshot_id, expected_node_count).await;
        match result {
            Ok(saved_at) => {
                tx.commit().await.map_err(|e| {
                    AppError::SnapshotPublication(format!("failed to commit publication: {e}"))
                })?;
                tracing::info!(
                    snapshot_id = %snapshot_id,
                    node_count = expected_node_count,
                    saved_at = %saved_at,
                    duration_ms = started.elapsed().as_millis() as u64,
                    "snapshot published"
                );
                Ok(saved_at)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                tracing::error!(
                    snapshot_id = %snapshot_id,
                    error_category = error.category(),
                    "snapshot publication failed"
                );
                Err(error)
            }
        }
    }

    async fn mark_failed(&self, snapshot_id: Uuid, message: &str) -> Result<(), AppError> {
        sqlx::query(
            r#"
            UPDATE node_snapshots
            SET status = 'failed', error_message = $2
            WHERE id = $1 AND status = 'loading'
            "#,
        )
        .bind(snapshot_id)
        .bind(message)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            AppError::PersistenceFailure(format!("failed to mark snapshot as failed: {e}"))
        })?;
        Ok(())
    }

    async fn active_snapshot_id(&self) -> Result<Option<Uuid>, AppError> {
        crate::features::nodes::infrastructure::postgres_reader::active_snapshot_id(&self.pool)
            .await
    }

    async fn cleanup_history(
        &self,
        retention_days: u32,
        minimum_successful: u32,
    ) -> Result<u64, AppError> {
        let rows = sqlx::query(
            r#"
            WITH active AS (
                SELECT snapshot_id AS id FROM active_snapshot WHERE singleton = TRUE
            ),
            recent AS (
                SELECT id
                FROM node_snapshots
                WHERE status = 'ready'
                  AND id <> (SELECT id FROM active)
                ORDER BY created_at DESC, id DESC
                LIMIT GREATEST($2::int - 1, 0)
            ),
            expired AS (
                SELECT s.id
                FROM node_snapshots s
                WHERE s.created_at < now() - make_interval(days => $1::int)
                  AND s.id <> (SELECT id FROM active)
                  AND s.id NOT IN (SELECT id FROM recent)
            )
            DELETE FROM node_snapshots s
            USING expired e
            WHERE s.id = e.id
            RETURNING s.id
            "#,
        )
        .bind(i32::try_from(retention_days).unwrap_or(i32::MAX))
        .bind(i32::try_from(minimum_successful).unwrap_or(i32::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::PersistenceFailure(format!("history cleanup failed: {e}")))?;

        let removed = rows.len() as u64;
        if removed > 0 {
            tracing::info!(
                removed_snapshots = removed,
                retention_days,
                minimum_successful_snapshots = minimum_successful,
                "history cleanup removed expired snapshots"
            );
        }
        Ok(removed)
    }
}

pub async fn publish_in_transaction(
    tx: &mut Transaction<'static, Postgres>,
    snapshot_id: Uuid,
    expected_node_count: usize,
) -> Result<DateTime<Utc>, AppError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(PUBLICATION_LOCK_KEY)
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            AppError::SnapshotPublication(format!("failed to acquire publication lock: {e}"))
        })?;

    let status: Option<String> =
        sqlx::query("SELECT status FROM node_snapshots WHERE id = $1 FOR UPDATE")
            .bind(snapshot_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| {
                AppError::SnapshotPublication(format!("failed to read snapshot status: {e}"))
            })?
            .and_then(|row| row.try_get::<String, usize>(0).ok());

    match status.as_deref() {
        Some("loading") => {}
        Some("ready") => {
            return Err(AppError::SnapshotPublication(format!(
                "snapshot {snapshot_id} is already published"
            )));
        }
        Some(other) => {
            return Err(AppError::SnapshotPublication(format!(
                "snapshot {snapshot_id} is in state {other:?}; cannot publish"
            )));
        }
        None => {
            return Err(AppError::SnapshotPublication(format!(
                "snapshot {snapshot_id} does not exist"
            )));
        }
    }

    let persisted: i64 =
        sqlx::query_scalar("SELECT count(*) FROM lightning_nodes WHERE snapshot_id = $1")
            .bind(snapshot_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| {
                AppError::SnapshotPublication(format!("failed to verify persisted nodes: {e}"))
            })?;

    let expected = i64::try_from(expected_node_count)
        .map_err(|_| AppError::SnapshotPublication("node count exceeds BIGINT".to_owned()))?;
    if persisted != expected {
        return Err(AppError::SnapshotPublication(format!(
            "persisted node count {persisted} does not match expected {expected}"
        )));
    }

    let saved_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        r#"
        UPDATE node_snapshots
        SET status = 'ready', saved_at = now(), node_count = $2
        WHERE id = $1
        RETURNING saved_at
        "#,
    )
    .bind(snapshot_id)
    .bind(
        i32::try_from(expected_node_count)
            .map_err(|_| AppError::SnapshotPublication("node count exceeds INT4".to_owned()))?,
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| AppError::SnapshotPublication(format!("failed to mark snapshot ready: {e}")))?;

    sqlx::query(
        r#"
        INSERT INTO active_snapshot (singleton, snapshot_id, activated_at)
        VALUES (TRUE, $1, now())
        ON CONFLICT (singleton) DO UPDATE
        SET snapshot_id = EXCLUDED.snapshot_id,
            activated_at = EXCLUDED.activated_at
        "#,
    )
    .bind(snapshot_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::SnapshotPublication(format!("failed to update active snapshot: {e}")))?;

    saved_at.ok_or_else(|| {
        AppError::SnapshotPublication("saved_at missing after publication".to_owned())
    })
}
