//! Shared integration-test harness: a real PostgreSQL via Testcontainers, a
//! local wiremock provider, and the application router exercised through
//! `tower::ServiceExt::oneshot` (no sockets needed).
// Common helpers are compiled into several test binaries; not every binary
// uses every helper.
#![allow(dead_code)]

use axum::{Router, body::Body, http::Request, http::StatusCode};
use nodes_svc::app::bootstrap::{AppComponents, build_components};
use nodes_svc::app::config::AppConfig;
use nodes_svc::features::nodes::commands::repository::SnapshotRepository;
use serde_json::Value;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use tower::util::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const RANKINGS_PATH: &str = "/api/v1/lightning/nodes/rankings/connectivity";

/// Fixture payload straight from the provider contract (3 nodes, includes an
/// empty alias, zero firstSeen and a large capacity).
pub const FIXTURE_BODY: &str = include_str!("../fixtures/mempool_response.json");

pub struct TestEnv {
    pub router: Router,
    pub provider: MockServer,
    pub components: AppComponents,
    #[expect(dead_code)] // container must stay alive for the whole test
    container: testcontainers_modules::testcontainers::ContainerAsync<Postgres>,
}

/// Build a provider payload with `count` unique nodes.
pub fn provider_json(count: usize) -> String {
    let nodes: Vec<String> = (0..count)
        .map(|i| {
            format!(
                r#"{{"publicKey":"pk{i:06}","alias":"Node {i}","channels":{i},"capacity":{capacity},"firstSeen":1522941222}}"#,
                capacity = i * 1_000 + 7
            )
        })
        .collect();
    format!("[{}]", nodes.join(","))
}

/// Mount a rankings response on the provider.
pub async fn mount_rankings(server: &MockServer, body: &str, status: u16, delay_ms: Option<u64>) {
    let mut template = ResponseTemplate::new(status).set_body_string(body);
    if let Some(delay) = delay_ms {
        template = template.set_delay(std::time::Duration::from_millis(delay));
    }
    Mock::given(method("GET"))
        .and(path(RANKINGS_PATH))
        .respond_with(template)
        .mount(server)
        .await;
}

fn test_config(
    provider_url: String,
    mempool_timeout_seconds: u64,
    include_saved_at: bool,
) -> AppConfig {
    let mut config = AppConfig::default();
    config.mempool.base_url = provider_url;
    config.mempool.timeout_seconds = mempool_timeout_seconds;
    config.synchronization.sync_on_startup = false; // tests drive sync explicitly
    config.api.include_saved_at = include_saved_at;
    config
}

/// Full stack: PostgreSQL container + provider mock + migrated app.
/// Panics are acceptable here: harness failures fail the test before any
/// assertion runs.
pub async fn start_stack(body: &str, status: u16, include_saved_at: bool) -> TestEnv {
    let container = Postgres::default()
        .start()
        .await
        .expect("postgres container");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("container port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

    let provider = MockServer::start().await;
    if !body.is_empty() {
        mount_rankings(&provider, body, status, None).await;
    }

    let config = test_config(provider.uri(), 5, include_saved_at);
    let components = build_components(config, Some(&url))
        .await
        .expect("app components");

    TestEnv {
        router: components.router.clone(),
        provider,
        components,
        container,
    }
}

/// Full stack with NO provider response mounted (provider "down").
pub async fn start_stack_provider_down(include_saved_at: bool) -> TestEnv {
    start_stack("", 0, include_saved_at).await
}

impl TestEnv {
    /// Issue a request against the router and return (status, json body).
    pub async fn get(&self, uri: &str) -> (StatusCode, Value) {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("router serves request");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("body");
        let value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).expect("json body")
        };
        (status, value)
    }

    pub async fn get_nodes(&self) -> (StatusCode, Value) {
        self.get("/nodes").await
    }

    /// Drive one full synchronization cycle against the wired provider.
    pub async fn run_sync(
        &self,
    ) -> Result<
        nodes_svc::features::nodes::commands::sync_nodes::SyncOutcome,
        nodes_svc::shared::error::AppError,
    > {
        self.components.sync_job.run_once().await
    }

    pub async fn active_snapshot_id(&self) -> Option<uuid::Uuid> {
        self.components
            .repository
            .active_snapshot_id()
            .await
            .expect("active snapshot")
    }

    pub async fn sql(&self, statement: &str) {
        sqlx::query(statement)
            .execute(&self.components.pools.write)
            .await
            .expect("sql");
    }
}
