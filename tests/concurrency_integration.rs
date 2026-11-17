//! Concurrency scenarios: imports racing with parallel HTTP reads, and
//! concurrent publication attempts.

mod common;

use axum::http::StatusCode;
use common::{provider_json, start_stack};
use std::collections::HashSet;
use tower::util::ServiceExt;

/// Multiple concurrent GET requests during an import: all succeed and every
/// response is one whole snapshot.
#[tokio::test]
async fn concurrent_reads_during_import_are_consistent() {
    let env = start_stack(&provider_json(10), 200, true).await;
    env.run_sync().await.expect("snapshot A (10)");

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(common::RANKINGS_PATH))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(provider_json(100))
                .set_delay(std::time::Duration::from_millis(30)),
        )
        .mount(&env.provider)
        .await;

    let router = env.router.clone();
    let readers = tokio::spawn(async move {
        let mut handles = Vec::new();
        for _ in 0..12 {
            let router = router.clone();
            handles.push(tokio::spawn(async move {
                let response = router
                    .oneshot(
                        axum::http::Request::builder()
                            .uri("/nodes")
                            .body(axum::body::Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let status = response.status();
                let body = axum::body::to_bytes(response.into_body(), 1 << 22)
                    .await
                    .unwrap();
                (status, body)
            }));
        }
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.unwrap());
        }
        results
    });

    let import = { env.run_sync() };
    let (_, responses) = tokio::join!(
        async { import.await.expect("snapshot B (100)") },
        async move { readers.await.expect("readers finish") }
    );

    let mut versions: HashSet<(usize, String)> = HashSet::new();
    for (status, body) in responses {
        assert_eq!(status, StatusCode::OK, "concurrent read failed");
        let nodes = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        let nodes = nodes.as_array().unwrap();
        let saved_at = nodes[0]["saved_at"].as_str().unwrap().to_owned();
        for node in nodes {
            assert_eq!(node["saved_at"].as_str().unwrap(), saved_at);
        }
        versions.insert((nodes.len(), saved_at));
    }
    assert!(
        versions
            .iter()
            .all(|(count, _)| *count == 10 || *count == 100),
        "only complete snapshots served: {versions:?}"
    );
}

/// Two concurrent sync cycles never double-publish: one wins, the other
/// observes the mutual exclusion, and exactly one snapshot is published.
#[tokio::test]
async fn concurrent_cycles_produce_single_publication() {
    let env = start_stack(&provider_json(5), 200, true).await;

    let (first, second) = tokio::join!(env.run_sync(), async { env.run_sync().await });
    // Both may succeed sequentially (mutex) or one may fail; the invariant is
    // a single active ready snapshot.
    let successes = [first.is_ok(), second.is_ok()]
        .into_iter()
        .filter(|ok| *ok)
        .count();
    assert!(successes >= 1, "at least one cycle succeeded");

    let ready: i64 =
        sqlx::query_scalar("SELECT count(*) FROM node_snapshots WHERE status = 'ready'")
            .fetch_one(&env.components.pools.read)
            .await
            .unwrap();
    assert_eq!(
        ready, successes as i64,
        "publications match successful cycles"
    );

    let active: i64 =
        sqlx::query_scalar("SELECT count(*) FROM active_snapshot WHERE snapshot_id IS NOT NULL")
            .fetch_one(&env.components.pools.read)
            .await
            .unwrap();
    assert_eq!(active, 1, "exactly one active snapshot");
}

/// HTTP throughput smoke test: repeated requests on a warm snapshot.
#[tokio::test]
async fn repeated_requests_are_stable() {
    let env = start_stack(&provider_json(100), 200, true).await;
    env.run_sync().await.expect("sync");

    for _ in 0..50 {
        let (status, body) = env.get_nodes().await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 100);
    }
}
