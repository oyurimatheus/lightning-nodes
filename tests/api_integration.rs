mod common;

use axum::http::StatusCode;
use common::{TestEnv, provider_json, start_stack, start_stack_provider_down};
use serde_json::{Value, json};
use tower::util::ServiceExt;
use wiremock::Mock;
use wiremock::matchers::{method, path};

async fn setup(body: &str) -> TestEnv {
    start_stack(body, 200, true).await
}

#[tokio::test]
async fn scenario_1_initial_synchronization() {
    let env = setup(&provider_json(3)).await;
    let outcome = env.run_sync().await.expect("sync succeeds");
    assert_eq!(outcome.node_count, 3);

    let (status, body) = env.get_nodes().await;
    assert_eq!(status, StatusCode::OK);
    let nodes = body.as_array().expect("array");
    assert_eq!(nodes.len(), 3);

    let first = &nodes[0];
    assert_eq!(first["public_key"], "pk000000");
    assert_eq!(first["alias"], "Node 0");

    assert_eq!(first["capacity"], "0.00000007");
    assert_eq!(first["first_seen"], "2018-04-05T15:13:42Z");
    assert!(
        first.get("saved_at").is_some(),
        "extended mode includes saved_at"
    );
}

#[tokio::test]
async fn nodes_endpoint_paginates_with_headers() {
    let env = setup(&provider_json(5)).await;
    env.run_sync().await.expect("sync");

    let (status, headers, body) = env.get_with_headers("/nodes?page=2&per_page=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["x-total-count"], "5");
    assert_eq!(headers["x-page"], "2");
    assert_eq!(headers["x-per-page"], "2");
    assert_eq!(headers["x-total-pages"], "3");
    let nodes = body.as_array().expect("array response");
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0]["public_key"], "pk000002");
    assert_eq!(nodes[1]["public_key"], "pk000003");

    let (_, _, last_page) = env.get_with_headers("/nodes?page=3&per_page=2").await;
    assert_eq!(last_page.as_array().unwrap().len(), 1);
    assert_eq!(last_page[0]["public_key"], "pk000004");

    let (_, _, beyond_last_page) = env.get_with_headers("/nodes?page=4&per_page=2").await;
    assert_eq!(beyond_last_page, json!([]));
}

#[tokio::test]
async fn nodes_endpoint_rejects_invalid_pagination() {
    let env = setup(&provider_json(1)).await;

    for uri in ["/nodes?page=0", "/nodes?per_page=0", "/nodes?per_page=101"] {
        let (status, _) = env.get(uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
    }
}

#[tokio::test]
async fn scenario_2_provider_timeout_preserves_active_snapshot() {
    let env = start_stack(&provider_json(2), 200, true).await;

    env.run_sync().await.expect("first sync");
    let snapshot_a = env.active_snapshot_id().await;

    env.provider.reset().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/lightning/nodes/rankings/connectivity"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(provider_json(5))
                .set_delay(std::time::Duration::from_millis(7_000)),
        )
        .mount(&env.provider)
        .await;

    let result = env.run_sync().await;
    assert!(result.is_err(), "timed-out cycle must fail");
    assert_eq!(
        env.active_snapshot_id().await,
        snapshot_a,
        "active snapshot unchanged"
    );

    let (status, body) = env.get_nodes().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.as_array().unwrap().len(),
        2,
        "snapshot A nodes still served"
    );
}

#[tokio::test]
async fn scenario_3_provider_500_preserves_data() {
    let env = setup(&provider_json(2)).await;
    env.run_sync().await.expect("first sync");
    let snapshot_a = env.active_snapshot_id().await;

    env.provider.reset().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/lightning/nodes/rankings/connectivity"))
        .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&env.provider)
        .await;

    assert!(
        env.run_sync().await.is_err(),
        "500 response must fail the cycle"
    );
    assert_eq!(env.active_snapshot_id().await, snapshot_a);
    let (status, body) = env.get_nodes().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn scenario_4_invalid_json_rejected() {
    let env = setup(&provider_json(2)).await;
    env.run_sync().await.expect("first sync");
    let snapshot_a = env.active_snapshot_id().await;

    env.provider.reset().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/lightning/nodes/rankings/connectivity"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("<html>oops</html>"))
        .mount(&env.provider)
        .await;

    assert!(env.run_sync().await.is_err(), "malformed payload must fail");
    assert_eq!(env.active_snapshot_id().await, snapshot_a);
    let (status, body) = env.get_nodes().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn scenario_9_saved_at_is_stable_and_shared() {
    let env = setup(&provider_json(5)).await;
    env.run_sync().await.expect("sync");

    let (status, body) = env.get_nodes().await;
    assert_eq!(status, StatusCode::OK);
    let nodes = body.as_array().unwrap();
    let saved_at_first = nodes[0]["saved_at"].clone();
    for node in nodes {
        assert_eq!(node["saved_at"], saved_at_first, "all nodes share saved_at");
    }

    let (_, body_again) = env.get_nodes().await;
    assert_eq!(
        body_again.as_array().unwrap()[0]["saved_at"],
        saved_at_first
    );
}

#[tokio::test]
async fn scenario_10_original_contract_without_saved_at() {
    let env = start_stack(&provider_json(2), 200, false).await;
    env.run_sync().await.expect("sync");

    let (status, body) = env.get_nodes().await;
    assert_eq!(status, StatusCode::OK);
    for node in body.as_array().unwrap() {
        let object = node.as_object().unwrap();
        let mut keys = object.keys().cloned().collect::<Vec<_>>();
        keys.sort();
        assert_eq!(keys, vec!["alias", "capacity", "first_seen", "public_key"]);
    }
}

#[tokio::test]
async fn scenario_12_server_starts_with_provider_down() {
    let env = start_stack_provider_down(true).await;
    let (status, _) = env.get("/health/live").await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = env.get("/health/ready").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "readiness depends on the database, not the provider"
    );

    let (status, headers, body) = env.get_with_headers("/nodes").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]), "empty database serves an empty array");
    assert_eq!(headers["x-total-count"], "0");
    assert_eq!(headers["x-page"], "1");
    assert_eq!(headers["x-per-page"], "20");
    assert_eq!(headers["x-total-pages"], "0");

    assert!(
        env.run_sync().await.is_err(),
        "provider down fails the cycle"
    );
    let (status, sync_status) = env.get("/sync/status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        sync_status["last_failure"]["category"],
        "provider_unexpected_status"
    );
}

#[tokio::test]
async fn scenario_13_reads_never_call_provider() {
    let env = setup(&provider_json(2)).await;
    env.run_sync().await.expect("sync");
    let requests_after_sync = env
        .provider
        .received_requests()
        .await
        .expect("requests")
        .len();

    for _ in 0..5 {
        let (status, _) = env.get_nodes().await;
        assert_eq!(status, StatusCode::OK);
    }
    assert_eq!(
        env.provider
            .received_requests()
            .await
            .expect("requests")
            .len(),
        requests_after_sync,
        "reads must not invoke the external provider"
    );
}

#[tokio::test]
async fn scenario_14_parallel_requests_during_import() {
    let env = start_stack(&provider_json(10), 200, true).await;
    env.run_sync()
        .await
        .expect("first sync publishes snapshot A (10 nodes)");

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(
            "/api/v1/lightning/nodes/rankings/connectivity",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(provider_json(15))
                .set_delay(std::time::Duration::from_millis(50)),
        )
        .mount(&env.provider)
        .await;

    let reads = {
        let router = env.router.clone();
        tokio::spawn(async move {
            let mut results = Vec::new();
            for _ in 0..30 {
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
                let body = axum::body::to_bytes(response.into_body(), 1 << 22)
                    .await
                    .unwrap();
                results.push(serde_json::from_slice::<Value>(&body).unwrap());
            }
            results
        })
    };

    let sync = { env.run_sync() };
    let (_, reads_result) = tokio::join!(async { sync.await.expect("second sync") }, reads);
    let reads = reads_result.expect("reader task finishes");

    let mut pairs = std::collections::HashSet::new();
    for read in &reads {
        let Value::Array(nodes) = read else {
            panic!("array response")
        };
        let saved_at = nodes[0]["saved_at"].as_str().expect("saved_at").to_owned();
        for node in nodes {
            assert_eq!(
                node["saved_at"].as_str().unwrap(),
                saved_at,
                "coherent snapshot per read"
            );
        }
        pairs.insert((saved_at, nodes.len()));
    }
    assert!(
        pairs.iter().all(|(_, count)| *count == 10 || *count == 15),
        "every read observes one whole snapshot: {pairs:?}"
    );
}

#[tokio::test]
async fn sync_status_reports_success() {
    let env = setup(&provider_json(2)).await;
    let outcome = env.run_sync().await.expect("sync");

    let (status, body) = env.get("/sync/status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "idle");
    assert_eq!(body["last_success"]["node_count"], 2);
    assert_eq!(body["last_snapshot_count"], 2);
    assert_eq!(
        body["active_snapshot_id"],
        Value::String(outcome.snapshot_id.to_string())
    );
}

#[tokio::test]
async fn empty_alias_preserved() {
    let env = setup(common::FIXTURE_BODY).await;
    env.run_sync().await.expect("sync");
    let (status, body) = env.get_nodes().await;
    assert_eq!(status, StatusCode::OK);
    let aliases: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["alias"].as_str().unwrap())
        .collect();
    assert!(
        aliases.contains(&""),
        "empty alias preserved verbatim: {aliases:?}"
    );
}
