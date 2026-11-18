use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

use crate::app::config::AppConfig;
use crate::shared::error::AppError;

pub struct DatabasePools {
    pub read: PgPool,
    pub write: PgPool,
}

pub async fn build(config: &AppConfig, url: &str) -> Result<DatabasePools, AppError> {
    let db = &config.database;
    Ok(DatabasePools {
        read: pool(url, db.read_pool_max_connections, db.acquire_timeout).await?,
        write: pool(url, db.write_pool_max_connections, db.acquire_timeout).await?,
    })
}

async fn pool(url: &str, max: u32, acquire: Duration) -> Result<PgPool, AppError> {
    PgPoolOptions::new()
        .max_connections(max)
        .acquire_timeout(acquire)
        .connect(url)
        .await
        .map_err(|e| AppError::DatabaseUnavailable(format!("failed to connect: {e}")))
}
