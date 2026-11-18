#![allow(dead_code)]

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use nodes_svc::app::bootstrap::{AppComponents, build_components};
use nodes_svc::app::config::AppConfig;
use nodes_svc::features::nodes::commands::repository::SnapshotRepository;
use serde_json::Value;
use tower::util::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const RANKINGS_PATH: &str = "/api/v1/lightning/nodes/rankings/connectivity";

pub const FIXTURE_BODY: &str = include_str!("../fixtures/mempool_response.json");

pub struct TestEnv {
    pub router: Router,
    pub provider: MockServer,
    pub components: AppComponents,
    pub database_url: String,
}

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
    config.mempool.timeout = std::time::Duration::from_secs(mempool_timeout_seconds);
    config.synchronization.sync_on_startup = false;
    config.api.include_saved_at = include_saved_at;
    config
}

pub fn test_database_url() -> String {
    dotenvy::dotenv().ok();
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("set TEST_DATABASE_URL to a PostgreSQL database for integration tests")
}

pub fn schema_url(database_url: &str, schema: &str) -> String {
    let separator = if database_url.contains('?') { '&' } else { '?' };
    format!("{database_url}{separator}options=-csearch_path%3D{schema}")
}

pub async fn create_test_schema(database_url: &str) -> (String, String) {
    let schema = format!("test_{}", uuid::Uuid::new_v4().simple());
    let admin = sqlx::PgPool::connect(database_url)
        .await
        .expect("connect to test PostgreSQL");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("create isolated test schema");
    admin.close().await;
    (schema_url(database_url, &schema), schema)
}

pub async fn start_stack(body: &str, status: u16, include_saved_at: bool) -> TestEnv {
    let base_url = test_database_url();
    let (url, _schema) = create_test_schema(&base_url).await;

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
        database_url: base_url,
    }
}

pub async fn start_stack_provider_down(include_saved_at: bool) -> TestEnv {
    start_stack("", 0, include_saved_at).await
}

impl TestEnv {
    pub async fn get(&self, uri: &str) -> (StatusCode, Value) {
        let (status, _, value) = self.get_with_headers(uri).await;
        (status, value)
    }

    pub async fn get_with_headers(&self, uri: &str) -> (StatusCode, HeaderMap, Value) {
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
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("body");
        let value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).expect("json body")
        };
        (status, headers, value)
    }

    pub async fn get_nodes(&self) -> (StatusCode, Value) {
        self.get("/nodes").await
    }

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
