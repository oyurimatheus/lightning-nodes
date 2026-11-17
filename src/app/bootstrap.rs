use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use uuid::Uuid;

use crate::app::config::AppConfig;
use crate::app::router;
use crate::app::state::AppState;
use crate::features::nodes::commands::repository::SnapshotRepository;
use crate::features::nodes::commands::sync_nodes::SyncNodesUseCase;
use crate::features::nodes::infrastructure::mempool_client::{MempoolClient, NodeProvider};
use crate::features::nodes::infrastructure::postgres_reader::PostgresNodeReader;
use crate::features::nodes::infrastructure::postgres_writer::PostgresSnapshotRepository;
use crate::features::nodes::queries::list_nodes::ListNodes;
use crate::features::synchronization::cleanup::HistoryCleanup;
use crate::features::synchronization::scheduler::{Scheduler, SyncJob};
use crate::features::synchronization::status::SyncStatusHandle;
use crate::shared::database::{self, DatabasePools};
use crate::shared::error::AppError;
/// Everything wired together, reused by `main` and integration tests.
pub struct AppComponents {
    pub router: Router,
    pub config: AppConfig,
    pub pools: DatabasePools,
    pub sync_job: Arc<SyncJob>,
    pub status: Arc<SyncStatusHandle>,
    pub repository: Arc<PostgresSnapshotRepository>,
    pub provider_url: String,
}

/// Connect to the database, apply migrations, and wire all features.
/// `database_url` overrides `DATABASE_URL` (used by integration tests).
pub async fn build_components(
    config: AppConfig,
    database_url: Option<&str>,
) -> Result<AppComponents, AppError> {
    let url = database_url
        .map(|s| s.to_owned())
        .or_else(|| config.database.url.clone())
        .ok_or_else(|| {
            AppError::Configuration(
                "database URL missing: set DATABASE_URL (see .env.example)".to_owned(),
            )
        })?;

    // Startup migration phase. sqlx tracks applied migrations in
    // `_sqlx_migrations` and takes a PostgreSQL advisory lock, so concurrent
    // starters cannot race; startup blocks until the schema is ready.
    let pools = database::build(&config, &url).await?;
    sqlx::migrate!("./migrations")
        .run(&pools.write)
        .await
        .map_err(|e| AppError::DatabaseUnavailable(format!("migrations failed: {e}")))?;
    tracing::info!("database migrations applied");

    let provider_url = config.mempool.base_url.clone();
    let provider: Arc<dyn NodeProvider> = Arc::new(MempoolClient::new(
        provider_url.clone(),
        Duration::from_secs(config.mempool.timeout_seconds),
    )?);
    let repository = Arc::new(PostgresSnapshotRepository::new(
        pools.write.clone(),
        config.synchronization.batch_size,
    ));
    let reader = Arc::new(PostgresNodeReader::new(pools.read.clone()));
    let reader_usecase_reader: Arc<dyn crate::features::nodes::queries::repository::NodeReader> =
        reader;
    let status = SyncStatusHandle::new();

    let sync: Arc<SyncNodesUseCase<dyn NodeProvider, dyn SnapshotRepository>> =
        Arc::new(SyncNodesUseCase::new(
            provider.clone(),
            repository.clone(),
            config.synchronization.reject_empty_response,
            status.clone(),
        ));
    let cleanup = HistoryCleanup::new(repository.clone());
    let sync_job = Arc::new(SyncJob::new(sync, cleanup, config.history.clone()));

    let state = AppState {
        reader: reader_usecase_reader.clone(),
        repository: repository.clone(),
        status: status.clone(),
        pool: pools.read.clone(),
        include_saved_at: config.api.include_saved_at,
    };
    let _ = ListNodes::new(reader_usecase_reader.clone()); // explicit read use case for future reuse
    let app_router = router::build(state);

    Ok(AppComponents {
        router: app_router,
        config,
        pools,
        sync_job,
        status,
        repository,
        provider_url,
    })
}

/// Spawn the background synchronization scheduler.
pub fn spawn_scheduler(components: &AppComponents) {
    let scheduler = Scheduler::new(
        components.sync_job.clone(),
        components.status.clone(),
        components.config.synchronization.interval_seconds,
        components.config.synchronization.sync_on_startup,
    );
    Arc::new(scheduler).spawn();
}

/// Production entry point: wire everything, spawn the scheduler, serve.
pub async fn run() -> Result<(), AppError> {
    crate::shared::telemetry::init();
    let config = AppConfig::load()?;
    let components = build_components(config, None).await?;
    spawn_scheduler(&components);

    let addr = format!(
        "{}:{}",
        components.config.server.host, components.config.server.port
    );
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| AppError::Configuration(format!("failed to bind {addr}: {e}")))?;
    tracing::info!(addr = %addr, "http server listening");

    axum::serve(listener, components.router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| AppError::Configuration(format!("server error: {e}")))?;
    tracing::info!("http server stopped gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler installation cannot fail");
        tokio::select! {
            _ = ctrl_c => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
    tracing::info!("shutdown signal received");
}

/// Convenience used by integration tests to expose the active snapshot id.
pub async fn active_snapshot_id(components: &AppComponents) -> Result<Option<Uuid>, AppError> {
    components.repository.active_snapshot_id().await
}
