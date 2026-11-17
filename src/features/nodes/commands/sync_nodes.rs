use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::features::nodes::commands::repository::SnapshotRepository;
use crate::features::nodes::domain::{LightningNode, validate_unique_public_keys};
use crate::features::nodes::infrastructure::mempool_client::{NodeProvider, ProviderNode};
use crate::features::synchronization::status::{LastSuccess, SyncStatusHandle};
use crate::shared::error::AppError;

/// Successful import result.
#[derive(Debug, Clone)]
pub struct SyncOutcome {
    pub snapshot_id: Uuid,
    pub saved_at: chrono::DateTime<chrono::Utc>,
    pub node_count: usize,
    pub duration_ms: u128,
}

/// Use case: fetch from the provider, validate the complete payload, persist
/// a new immutable snapshot, and publish it atomically. Any failure keeps the
/// previously active snapshot serving and records the error.
pub struct SyncNodesUseCase<P: NodeProvider + ?Sized, R: SnapshotRepository + ?Sized> {
    provider: Arc<P>,
    repository: Arc<R>,
    reject_empty_response: bool,
    status: Arc<SyncStatusHandle>,
}

impl<P: NodeProvider + ?Sized, R: SnapshotRepository + ?Sized> SyncNodesUseCase<P, R> {
    pub fn new(
        provider: Arc<P>,
        repository: Arc<R>,
        reject_empty_response: bool,
        status: Arc<SyncStatusHandle>,
    ) -> Self {
        Self {
            provider,
            repository,
            reject_empty_response,
            status,
        }
    }

    pub async fn execute(&self) -> Result<SyncOutcome, AppError> {
        let started = Instant::now();
        let sync_id = Uuid::new_v4();
        self.status.begin().await;
        tracing::info!(sync_id = %sync_id, "synchronization started");

        let result = self.run(sync_id).await;
        match result {
            Ok(outcome) => {
                self.status
                    .record_success({
                        let node_count = outcome.node_count as u64;
                        let at = outcome.saved_at;
                        let snapshot_id = outcome.snapshot_id;
                        LastSuccess {
                            at,
                            snapshot_id,
                            node_count,
                        }
                    })
                    .await;
                tracing::info!(
                    sync_id = %sync_id,
                    snapshot_id = %outcome.snapshot_id,
                    nodes_received = outcome.node_count,
                    nodes_persisted = outcome.node_count,
                    duration_ms = started.elapsed().as_millis(),
                    "synchronization succeeded"
                );
                Ok(outcome)
            }
            Err(error) => {
                self.status.record_failure(&error).await;
                tracing::error!(
                    sync_id = %sync_id,
                    error_category = error.category(),
                    duration_ms = started.elapsed().as_millis(),
                    "synchronization failed"
                );
                Err(error)
            }
        }
    }

    async fn run(&self, sync_id: Uuid) -> Result<SyncOutcome, AppError> {
        let started = Instant::now();
        // 1. Fetch from the provider.
        let provider_nodes = self.provider.fetch_nodes().await?;
        self.status
            .record_payload_size(provider_nodes.len() as u64)
            .await;
        tracing::debug!(sync_id = %sync_id, nodes_received = provider_nodes.len(), "provider payload received");

        // 2. Validate the complete payload before touching the active snapshot.
        //    An unexpected empty response is a failure by default so a provider
        //    incident cannot wipe valid data.
        if provider_nodes.is_empty() && self.reject_empty_response {
            return Err(AppError::InvalidProviderPayload(
                "provider returned an empty response; refusing to import".to_owned(),
            ));
        }
        let nodes = self.convert(&provider_nodes)?;
        validate_unique_public_keys(&nodes)?;

        // 3. Create the new immutable snapshot and persist node rows in batches.
        let snapshot_id = self.repository.create_loading_snapshot().await?;
        let write_result = self.repository.write_nodes(snapshot_id, &nodes).await;
        let outcome = match write_result {
            Ok(()) => self.repository.publish(snapshot_id, nodes.len()).await,
            Err(error) => Err(error),
        };

        match outcome {
            Ok(saved_at) => Ok(SyncOutcome {
                snapshot_id,
                saved_at,
                node_count: nodes.len(),
                duration_ms: started.elapsed().as_millis(),
            }),
            Err(error) => {
                // Never publish a partial snapshot; record the failure and keep
                // the previously active snapshot intact. If even marking the
                // snapshot as failed fails (e.g. database down), log it: the
                // `loading` snapshot is harmless and can be reaped later.
                if let Err(mark_error) = self
                    .repository
                    .mark_failed(snapshot_id, &error.to_string())
                    .await
                {
                    tracing::error!(
                        sync_id = %sync_id,
                        snapshot_id = %snapshot_id,
                        error_category = mark_error.category(),
                        "failed to mark snapshot as failed; it stays 'loading'"
                    );
                }
                Err(error)
            }
        }
    }

    fn convert(&self, provider_nodes: &[ProviderNode]) -> Result<Vec<LightningNode>, AppError> {
        let mut nodes = Vec::with_capacity(provider_nodes.len());
        let mut seen = HashSet::with_capacity(provider_nodes.len());
        for provider_node in provider_nodes {
            let node = LightningNode::new(
                provider_node.public_key.clone(),
                provider_node.alias.clone(),
                provider_node.capacity,
                provider_node.first_seen,
            )
            .map_err(|e| {
                AppError::InvalidProviderPayload(format!(
                    "invalid node {}: {e}",
                    provider_node.public_key
                ))
            })?;
            if !seen.insert(node.public_key.clone()) {
                return Err(AppError::DuplicateNode(node.public_key.clone()));
            }
            nodes.push(node);
        }
        Ok(nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::synchronization::status::SyncState;
    use std::sync::Mutex;

    struct FakeProvider {
        result: Mutex<Result<Vec<ProviderNode>, AppError>>,
    }
    impl FakeProvider {
        fn ok(nodes: Vec<ProviderNode>) -> Self {
            Self {
                result: Mutex::new(Ok(nodes)),
            }
        }
        fn failing(error: AppError) -> Self {
            Self {
                result: Mutex::new(Err(error)),
            }
        }
    }

    #[async_trait::async_trait]
    impl NodeProvider for FakeProvider {
        async fn fetch_nodes(&self) -> Result<Vec<ProviderNode>, AppError> {
            self.result.lock().unwrap().clone()
        }
    }

    fn provider_node(key: &str, capacity: u64, first_seen: i64) -> ProviderNode {
        ProviderNode {
            public_key: key.to_owned(),
            alias: "ACINQ".to_owned(),
            channels: Some(1),
            capacity,
            first_seen,
            updated_at: None,
            city: None,
            country: None,
        }
    }

    struct FakeRepo {
        fail_write: bool,
        fail_publish: bool,
        calls: Mutex<Vec<String>>,
        active: Mutex<Option<Uuid>>,
    }

    impl FakeRepo {
        fn new() -> Self {
            Self {
                fail_write: false,
                fail_publish: false,
                calls: Mutex::new(Vec::new()),
                active: Mutex::new(None),
            }
        }

        fn log(&self, event: &str) {
            self.calls.lock().unwrap().push(event.to_owned());
        }
    }

    #[async_trait::async_trait]
    impl SnapshotRepository for FakeRepo {
        async fn create_loading_snapshot(&self) -> Result<Uuid, AppError> {
            self.log("create_loading");
            Ok(Uuid::new_v4())
        }

        async fn write_nodes(&self, _id: Uuid, nodes: &[LightningNode]) -> Result<(), AppError> {
            self.log("write_nodes");
            if self.fail_write {
                return Err(AppError::PersistenceFailure("write failed".to_owned()));
            }
            let _ = nodes.len();
            Ok(())
        }

        async fn publish(
            &self,
            id: Uuid,
            _count: usize,
        ) -> Result<chrono::DateTime<chrono::Utc>, AppError> {
            self.log("publish");
            if self.fail_publish {
                return Err(AppError::SnapshotPublication("publish failed".to_owned()));
            }
            *self.active.lock().unwrap() = Some(id);
            Ok(chrono::Utc::now())
        }

        async fn mark_failed(&self, _id: Uuid, _message: &str) -> Result<(), AppError> {
            self.log("mark_failed");
            Ok(())
        }

        async fn active_snapshot_id(&self) -> Result<Option<Uuid>, AppError> {
            Ok(*self.active.lock().unwrap())
        }

        async fn cleanup_history(&self, _days: u32, _minimum: u32) -> Result<u64, AppError> {
            Ok(0)
        }
    }

    async fn use_case(
        provider: FakeProvider,
        repo: FakeRepo,
        reject_empty: bool,
    ) -> (
        SyncNodesUseCase<FakeProvider, FakeRepo>,
        Arc<FakeRepo>,
        Arc<SyncStatusHandle>,
    ) {
        let status = SyncStatusHandle::new();
        let repo = Arc::new(repo);
        let uc = SyncNodesUseCase::new(
            Arc::new(provider),
            repo.clone(),
            reject_empty,
            status.clone(),
        );
        (uc, repo, status)
    }

    #[tokio::test]
    async fn successful_synchronization_publishes_snapshot() {
        let (uc, repo, status) = use_case(
            FakeProvider::ok(vec![
                provider_node("pk1", 1_000, 0),
                provider_node("pk2", 2_000, 1522941222),
            ]),
            FakeRepo::new(),
            true,
        )
        .await;
        let outcome = uc.execute().await.unwrap();
        assert_eq!(outcome.node_count, 2);
        assert_eq!(repo.active.lock().unwrap().unwrap(), outcome.snapshot_id);
        assert_eq!(status.snapshot().await.last_snapshot_count, Some(2));
        let calls = repo.calls.lock().unwrap();
        assert_eq!(
            calls.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["create_loading", "write_nodes", "publish"]
        );
    }

    #[tokio::test]
    async fn failed_provider_keeps_active_snapshot_and_records_failure() {
        let (uc, repo, status) = use_case(
            FakeProvider::failing(AppError::ProviderTimeout),
            FakeRepo::new(),
            true,
        )
        .await;
        assert!(uc.execute().await.is_err());
        // No snapshot was even created.
        assert!(repo.calls.lock().unwrap().is_empty());
        let snapshot = status.snapshot().await;
        assert_eq!(snapshot.state, SyncState::Idle);
        assert_eq!(snapshot.last_failure.unwrap().category, "provider_timeout");
    }

    #[tokio::test]
    async fn failed_persistence_marks_snapshot_failed_and_preserves_active() {
        let mut repo = FakeRepo::new();
        repo.fail_write = true;
        let (uc, repo, status) = use_case(
            FakeProvider::ok(vec![provider_node("pk1", 1, 0)]),
            repo,
            true,
        )
        .await;
        assert!(uc.execute().await.is_err());
        {
            let calls = repo.calls.lock().unwrap();
            assert!(calls.contains(&"mark_failed".to_owned()));
            assert!(!calls.contains(&"publish".to_owned()));
        }
        assert!(status.snapshot().await.last_failure.is_some());
    }

    #[tokio::test]
    async fn failed_publication_marks_snapshot_failed() {
        let mut repo = FakeRepo::new();
        repo.fail_publish = true;
        let (uc, repo, _) = use_case(
            FakeProvider::ok(vec![provider_node("pk1", 1, 0)]),
            repo,
            true,
        )
        .await;
        assert!(uc.execute().await.is_err());
        let calls = repo.calls.lock().unwrap();
        assert!(calls.contains(&"publish".to_owned()));
        assert!(calls.contains(&"mark_failed".to_owned()));
    }

    #[tokio::test]
    async fn empty_response_rejected_when_policy_enabled() {
        let (uc, repo, _) = use_case(FakeProvider::ok(vec![]), FakeRepo::new(), true).await;
        let err = uc.execute().await.unwrap_err();
        assert!(matches!(err, AppError::InvalidProviderPayload(_)));
        assert!(repo.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn empty_response_allowed_when_policy_disabled() {
        let (uc, repo, _) = use_case(FakeProvider::ok(vec![]), FakeRepo::new(), false).await;
        let outcome = uc.execute().await.unwrap();
        assert_eq!(outcome.node_count, 0);
        assert!(repo.calls.lock().unwrap().contains(&"publish".to_owned()));
    }

    #[tokio::test]
    async fn duplicate_public_keys_rejected_before_persistence() {
        let (uc, repo, _) = use_case(
            FakeProvider::ok(vec![provider_node("pk1", 1, 0), provider_node("pk1", 2, 0)]),
            FakeRepo::new(),
            true,
        )
        .await;
        let err = uc.execute().await.unwrap_err();
        assert!(matches!(err, AppError::DuplicateNode(_)));
        assert!(repo.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn invalid_timestamp_rejected() {
        let (uc, repo, _) = use_case(
            FakeProvider::ok(vec![provider_node("pk1", 1, i64::MAX)]),
            FakeRepo::new(),
            true,
        )
        .await;
        let err = uc.execute().await.unwrap_err();
        assert!(matches!(err, AppError::InvalidProviderPayload(_)));
        assert!(repo.calls.lock().unwrap().is_empty());
    }
}
