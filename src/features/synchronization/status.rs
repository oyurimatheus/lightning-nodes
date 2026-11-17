use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::shared::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Idle,
    Syncing,
}

#[derive(Debug, Clone, Serialize)]
pub struct LastSuccess {
    /// `saved_at` of the snapshot published in that synchronization.
    pub at: DateTime<Utc>,
    pub snapshot_id: Uuid,
    pub node_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LastFailure {
    pub at: DateTime<Utc>,
    pub category: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    pub state: SyncState,
    pub last_success: Option<LastSuccess>,
    pub last_failure: Option<LastFailure>,
    /// Node count of the last attempt (success or failure payload size).
    pub last_snapshot_count: Option<u64>,
}

impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            state: SyncState::Idle,
            last_success: None,
            last_failure: None,
            last_snapshot_count: None,
        }
    }
}

/// Thread-safe synchronization status shared between the scheduler and the
/// `/sync/status` endpoint.
#[derive(Default)]
pub struct SyncStatusHandle {
    inner: RwLock<SyncStatus>,
}

impl SyncStatusHandle {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn snapshot(&self) -> SyncStatus {
        self.inner.read().await.clone()
    }

    pub async fn begin(&self) {
        let mut status = self.inner.write().await;
        status.state = SyncState::Syncing;
    }

    pub async fn record_success(&self, success: LastSuccess) {
        let mut status = self.inner.write().await;
        status.state = SyncState::Idle;
        status.last_snapshot_count = Some(success.node_count);
        status.last_success = Some(success);
    }

    pub async fn record_failure(&self, error: &AppError) {
        let mut status = self.inner.write().await;
        status.state = SyncState::Idle;
        status.last_failure = Some(LastFailure {
            at: Utc::now(),
            category: error.category().to_owned(),
            message: error.to_string(),
        });
    }

    pub async fn record_payload_size(&self, count: u64) {
        self.inner.write().await.last_snapshot_count = Some(count);
    }
}
