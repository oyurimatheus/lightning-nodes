use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::features::nodes::domain::LightningNode;
use crate::features::nodes::queries::repository::{ActiveNode, NodeReader};
use crate::shared::error::AppError;

/// PostgreSQL read adapter. Uses the covering index
/// `(snapshot_id, public_key) INCLUDE (alias, capacity_sats, first_seen)`,
/// selects only required columns, joins through the singleton active pointer,
/// and returns a deterministic order. Single query: no N+1.
pub struct PostgresNodeReader {
    pool: PgPool,
}

impl PostgresNodeReader {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Debug, FromRow)]
struct NodeRow {
    public_key: String,
    alias: String,
    capacity_sats: i64,
    first_seen: DateTime<Utc>,
    saved_at: DateTime<Utc>,
}

#[async_trait]
impl NodeReader for PostgresNodeReader {
    async fn list_active_nodes(&self) -> Result<Vec<ActiveNode>, AppError> {
        let rows = sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT
                n.public_key,
                n.alias,
                n.capacity_sats,
                n.first_seen,
                s.saved_at
            FROM active_snapshot a
            JOIN node_snapshots s
                ON s.id = a.snapshot_id
            JOIN lightning_nodes n
                ON n.snapshot_id = s.id
            WHERE a.singleton = TRUE
            ORDER BY n.public_key
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseUnavailable(format!("failed to read nodes: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|row| ActiveNode {
                node: LightningNode {
                    public_key: row.public_key,
                    alias: row.alias,
                    capacity_sats: row.capacity_sats,
                    first_seen: row.first_seen,
                },
                saved_at: row.saved_at,
            })
            .collect())
    }
}

/// Convenience for tests/tools: the active snapshot identifier.
pub async fn active_snapshot_id(pool: &PgPool) -> Result<Option<Uuid>, AppError> {
    sqlx::query_scalar::<_, Option<Uuid>>(
        r#"SELECT snapshot_id FROM active_snapshot WHERE singleton = TRUE"#,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::DatabaseUnavailable(format!("failed to read active snapshot: {e}")))
}
