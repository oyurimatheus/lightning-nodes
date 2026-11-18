use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::features::nodes::domain::LightningNode;
use crate::features::nodes::queries::repository::{ActiveNode, NodePage, NodeReader};
use crate::shared::error::AppError;

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
    public_key: Option<String>,
    alias: Option<String>,
    capacity_sats: Option<i64>,
    first_seen: Option<DateTime<Utc>>,
    saved_at: Option<DateTime<Utc>>,
    total_count: i64,
}

#[async_trait]
impl NodeReader for PostgresNodeReader {
    async fn list_active_nodes(&self, limit: i64, offset: i64) -> Result<NodePage, AppError> {
        let rows = sqlx::query_as::<_, NodeRow>(
            r#"
            WITH active_version AS MATERIALIZED (
                SELECT s.id, s.saved_at
                FROM active_snapshot a
                JOIN node_snapshots s ON s.id = a.snapshot_id
                WHERE a.singleton = TRUE
            ),
            total AS (
                SELECT COUNT(n.public_key)::BIGINT AS total_count
                FROM active_version v
                LEFT JOIN lightning_nodes n ON n.snapshot_id = v.id
            ),
            page AS (
                SELECT
                    n.public_key,
                    n.alias,
                    n.capacity_sats,
                    n.first_seen,
                    v.saved_at
                FROM active_version v
                JOIN lightning_nodes n ON n.snapshot_id = v.id
                ORDER BY n.public_key
                LIMIT $1 OFFSET $2
            )
            SELECT
                page.public_key,
                page.alias,
                page.capacity_sats,
                page.first_seen,
                page.saved_at,
                total.total_count
            FROM total
            LEFT JOIN page ON TRUE
            ORDER BY page.public_key NULLS LAST
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseUnavailable(format!("failed to read nodes: {e}")))?;

        let mut nodes = Vec::with_capacity(rows.len());
        let mut total_count = 0;
        for row in rows {
            total_count = u64::try_from(row.total_count).map_err(|_| {
                AppError::DatabaseUnavailable("node count cannot be negative".to_owned())
            })?;

            let Some(public_key) = row.public_key else {
                continue;
            };
            let node = LightningNode {
                public_key,
                alias: row.alias.ok_or_else(|| {
                    AppError::DatabaseUnavailable("node page is missing an alias".to_owned())
                })?,
                capacity_sats: row.capacity_sats.ok_or_else(|| {
                    AppError::DatabaseUnavailable("node page is missing a capacity".to_owned())
                })?,
                first_seen: row.first_seen.ok_or_else(|| {
                    AppError::DatabaseUnavailable("node page is missing first_seen".to_owned())
                })?,
            };
            let saved_at = row.saved_at.ok_or_else(|| {
                AppError::DatabaseUnavailable("node page is missing saved_at".to_owned())
            })?;
            nodes.push(ActiveNode { node, saved_at });
        }

        Ok(NodePage { nodes, total_count })
    }
}

pub async fn active_snapshot_id(pool: &PgPool) -> Result<Option<Uuid>, AppError> {
    sqlx::query_scalar::<_, Option<Uuid>>(
        r#"SELECT snapshot_id FROM active_snapshot WHERE singleton = TRUE"#,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::DatabaseUnavailable(format!("failed to read active snapshot: {e}")))
}
