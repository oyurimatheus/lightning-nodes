use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::config::HistoryConfig;
use crate::features::nodes::commands::repository::SnapshotRepository;
use crate::features::nodes::commands::sync_nodes::{SyncNodesUseCase, SyncOutcome};
use crate::features::nodes::infrastructure::mempool_client::NodeProvider;
use crate::features::synchronization::cleanup::HistoryCleanup;
use crate::features::synchronization::status::SyncStatusHandle;
use crate::shared::error::AppError;

pub type SyncUseCase = SyncNodesUseCase<dyn NodeProvider, dyn SnapshotRepository>;

/// One synchronization cycle: a single import (mutually excluded in-process,
/// advisory-locked across instances) followed by best-effort history cleanup.
pub struct SyncJob {
    sync: Arc<SyncUseCase>,
    cleanup: HistoryCleanup,
    history: HistoryConfig,
    mutex: tokio::sync::Mutex<()>,
}

impl SyncJob {
    pub fn new(sync: Arc<SyncUseCase>, cleanup: HistoryCleanup, history: HistoryConfig) -> Self {
        Self {
            sync,
            cleanup,
            history,
            mutex: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn run_once(&self) -> Result<SyncOutcome, AppError> {
        let _guard = self.mutex.lock().await;
        let outcome = self.sync.execute().await?;
        if let Err(error) = self
            .cleanup
            .run(
                self.history.retention_days,
                self.history.minimum_successful_snapshots,
            )
            .await
        {
            // Cleanup is best-effort; it must never fail the import.
            tracing::warn!(error_category = error.category(), "history cleanup failed");
        }
        Ok(outcome)
    }
}

/// Bounded exponential backoff with jitter for provider-side errors.
/// Infrastructure errors (e.g. database down) abort the cycle early.
pub async fn run_with_backoff(
    job: &SyncJob,
    max_attempts: u32,
    status: &SyncStatusHandle,
) -> Result<SyncOutcome, AppError> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match job.run_once().await {
            Ok(outcome) => return Ok(outcome),
            Err(error) if error.is_provider_error() && attempt < max_attempts => {
                let base = Duration::from_secs(2u64.saturating_pow(attempt - 1));
                let jitter = Duration::from_millis(
                    (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos() % 250)
                        .unwrap_or(0)) as u64,
                );
                let wait = base + jitter;
                tracing::warn!(
                    attempt,
                    max_attempts,
                    wait_ms = wait.as_millis() as u64,
                    error_category = error.category(),
                    "synchronization attempt failed; retrying with backoff"
                );
                tokio::time::sleep(wait).await;
            }
            Err(error) => {
                status.record_failure(&error).await;
                return Err(error);
            }
        }
    }
}

/// Sequential scheduler: one cycle at startup (when enabled), then a fixed
/// interval. Missed ticks are skipped rather than queued to avoid backlog.
pub struct Scheduler {
    job: Arc<SyncJob>,
    status: Arc<SyncStatusHandle>,
    interval_seconds: u64,
    sync_on_startup: bool,
}

impl Scheduler {
    pub fn new(
        job: Arc<SyncJob>,
        status: Arc<SyncStatusHandle>,
        interval_seconds: u64,
        sync_on_startup: bool,
    ) -> Self {
        Self {
            job,
            status,
            interval_seconds,
            sync_on_startup,
        }
    }

    /// Spawn the background loop; returns immediately so the HTTP server is
    /// available before the first import completes.
    pub fn spawn(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(self.interval_seconds));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut first = true;
            loop {
                ticker.tick().await;
                if first {
                    first = false;
                    if !self.sync_on_startup {
                        tracing::debug!("startup synchronization disabled");
                        continue;
                    }
                }
                tracing::info!("synchronization cycle starting");
                let started = Instant::now();
                match run_with_backoff(&self.job, 3, &self.status).await {
                    Ok(outcome) => tracing::info!(
                        snapshot_id = %outcome.snapshot_id,
                        nodes_persisted = outcome.node_count,
                        duration_ms = started.elapsed().as_millis() as u64,
                        "synchronization cycle finished"
                    ),
                    Err(error) => tracing::error!(
                        error_category = error.category(),
                        duration_ms = started.elapsed().as_millis() as u64,
                        "synchronization cycle failed; previous snapshot stays active"
                    ),
                }
            }
        });
    }
}
