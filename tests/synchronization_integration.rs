//! Synchronization lifecycle scenarios: replacement, history, atomic
//! publication and historical cleanup.

mod common;

use axum::http::StatusCode;
use common::{provider_json, start_stack};
use nodes_svc::features::nodes::commands::repository::SnapshotRepository;
use std::collections::HashSet;
use tower::util::ServiceExt;

/// Scenario 6 — Snapshot replacement: A (10 nodes) replaced by B (15 nodes).
#[tokio::test]
async fn scenario_6_snapshot_replacement() {
    let env = start_stack(&provider_json(10), 200, true).await;
    env.run_sync().await.expect("snapshot A");
    let (_, body) = env.get_nodes().await;
    assert_eq!(body.as_array().unwrap().len(), 10);

    // New payload of 15 nodes.
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(common::RANKINGS_PATH))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(provider_json(15)))
        .mount(&env.provider)
        .await;
    env.run_sync().await.expect("snapshot B");

    let (status, body) = env.get_nodes().await;
    assert_eq!(status, StatusCode::OK);
    let nodes = body.as_array().unwrap();
    assert_eq!(nodes.len(), 15);
    assert_eq!(nodes[0]["public_key"], "pk000000");
    assert!(
        nodes.iter().any(|n| n["public_key"] == "pk000014"),
        "serving snapshot B"
    );
}

/// Scenario 7 — Historical snapshots: A still exists after B becomes active.
#[tokio::test]
async fn scenario_7_history_is_retained() {
    let env = start_stack(&provider_json(10), 200, true).await;
    env.run_sync().await.expect("snapshot A");
    let snapshot_a = env.active_snapshot_id().await.expect("A");

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(common::RANKINGS_PATH))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(provider_json(15)))
        .mount(&env.provider)
        .await;
    env.run_sync().await.expect("snapshot B");
    let snapshot_b = env.active_snapshot_id().await.expect("B");
    assert_ne!(snapshot_a, snapshot_b);

    // Both ready snapshots exist in history.
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM node_snapshots WHERE status = 'ready'")
            .fetch_one(&env.components.pools.read)
            .await
            .expect("count ready snapshots");
    assert_eq!(count, 2);

    // Snapshot A's nodes are still there.
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM lightning_nodes WHERE snapshot_id = $1")
            .bind(snapshot_a)
            .fetch_one(&env.components.pools.read)
            .await
            .expect("count snapshot A nodes");
    assert_eq!(count, 10);
}

/// Scenario 8 — Atomic publication: reads during an import observe exactly
/// one coherent snapshot, never a mixture of two versions.
#[tokio::test]
async fn scenario_8_publication_is_atomic_for_readers() {
    let env = start_stack(&provider_json(10), 200, true).await;
    env.run_sync().await.expect("snapshot A");

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(common::RANKINGS_PATH))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(provider_json(15))
                .set_delay(std::time::Duration::from_millis(20)),
        )
        .mount(&env.provider)
        .await;

    let router = env.router.clone();
    let reader = tokio::spawn(async move {
        let mut seen: HashSet<(String, usize)> = HashSet::new();
        for _ in 0..40 {
            let response = router
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri("/nodes")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), 1 << 22)
                .await
                .unwrap();
            let nodes = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
            let nodes = nodes.as_array().unwrap();
            let saved_at = nodes[0]["saved_at"].as_str().unwrap().to_owned();
            for node in nodes {
                assert_eq!(
                    node["saved_at"].as_str().unwrap(),
                    saved_at,
                    "a single read never mixes snapshots"
                );
            }
            seen.insert((saved_at, nodes.len()));
        }
        seen
    });

    let (_, reader_result) =
        tokio::join!(async { env.run_sync().await.expect("snapshot B") }, reader);
    let seen = reader_result.expect("reader task finishes");
    // Every observed (saved_at, count) pair corresponds to one whole snapshot.
    assert!(
        seen.iter().all(|(_, count)| *count == 10 || *count == 15),
        "only complete snapshots were served: {seen:?}"
    );
    assert_eq!(
        seen.len(),
        2,
        "both versions were observed during the transition"
    );
}

/// Scenario 11 — Historical cleanup: expired snapshots removed; active and
/// the configured minimum history preserved.
#[tokio::test]
async fn scenario_11_cleanup_respects_active_and_minimum() {
    let env = start_stack(&provider_json(10), 200, true).await;
    env.run_sync().await.expect("snapshot A"); // active

    for _ in 0..5 {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(common::RANKINGS_PATH))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string(provider_json(11 + rand_tick())),
            )
            .mount(&env.provider)
            .await;
        env.run_sync().await.expect("snapshot");
    }
    let active = env.active_snapshot_id().await.expect("active");

    // Force age beyond retention: pretend every snapshot was created long ago.
    env.sql("UPDATE node_snapshots SET created_at = now() - interval '400 days'")
        .await;

    let removed = env
        .components
        .repository
        .cleanup_history(30, 3)
        .await
        .expect("cleanup runs");

    assert!(removed > 0, "expired snapshots were removed");

    // Active snapshot untouched.
    assert_eq!(env.active_snapshot_id().await, Some(active));

    // At least `minimum_successful` ready snapshots remain (active counts).
    let ready: i64 =
        sqlx::query_scalar("SELECT count(*) FROM node_snapshots WHERE status = 'ready'")
            .fetch_one(&env.components.pools.read)
            .await
            .expect("count ready");
    assert_eq!(ready, 3, "minimum history preserved, remainder removed");

    // `GET /nodes` still serves the active snapshot.
    let (status, body) = env.get_nodes().await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.as_array().unwrap().is_empty());
}

/// Cheap per-run payload variation without extra dependencies.
fn rand_tick() -> usize {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.subsec_nanos() as usize) % 4)
        .unwrap_or(1)
}

/// Cleanup must never delete the active snapshot even when it is old.
#[tokio::test]
async fn cleanup_never_removes_active_snapshot() {
    let env = start_stack(&provider_json(2), 200, true).await;
    env.run_sync().await.expect("snapshot A");
    let active = env.active_snapshot_id().await.expect("active");

    env.sql("UPDATE node_snapshots SET created_at = now() - interval '400 days'")
        .await;
    let removed = env
        .components
        .repository
        .cleanup_history(30, 3)
        .await
        .expect("cleanup");
    assert!(removed > 0);
    assert_eq!(env.active_snapshot_id().await, Some(active));

    let nodes: i64 = sqlx::query_scalar("SELECT count(*) FROM lightning_nodes")
        .fetch_one(&env.components.pools.read)
        .await
        .expect("count nodes");
    assert_eq!(nodes, 2, "active snapshot nodes survive cleanup");
}
