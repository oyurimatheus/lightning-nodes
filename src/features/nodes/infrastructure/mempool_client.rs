use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

use crate::shared::error::AppError;

/// DTO for the mempool.space connectivity ranking response.
///
/// Only the fields the application consumes are required; extra fields are
/// ignored (provider may evolve the payload). Monetary and time values stay
/// in integer form — never floats.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderNode {
    pub public_key: String,
    pub alias: String,
    #[serde(default)]
    pub channels: Option<u32>,
    pub capacity: u64,
    pub first_seen: i64,
    #[serde(default)]
    pub updated_at: Option<i64>,
    #[serde(default)]
    pub city: Option<serde_json::Value>,
    #[serde(default)]
    pub country: Option<serde_json::Value>,
}

/// Source of provider node data, isolated behind a trait so the sync use case
/// can be unit-tested with fakes and integration-tested with wiremock.
#[async_trait]
pub trait NodeProvider: Send + Sync {
    async fn fetch_nodes(&self) -> Result<Vec<ProviderNode>, AppError>;
}

pub struct MempoolClient {
    base_url: String,
    http: reqwest::Client,
}

impl MempoolClient {
    pub fn new(base_url: String, timeout: Duration) -> Result<Self, AppError> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout)
            .build()
            .map_err(|e| AppError::Configuration(format!("failed to build HTTP client: {e}")))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            http,
        })
    }
}

#[async_trait]
impl NodeProvider for MempoolClient {
    async fn fetch_nodes(&self) -> Result<Vec<ProviderNode>, AppError> {
        let url = format!(
            "{}/api/v1/lightning/nodes/rankings/connectivity",
            self.base_url
        );
        tracing::debug!(url = %url, "fetching provider nodes");

        let response = self.http.get(&url).send().await.map_err(|e| {
            if e.is_timeout() {
                AppError::ProviderTimeout
            } else {
                AppError::ProviderUnavailable(e.to_string())
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            return Err(AppError::ProviderUnexpectedStatus {
                status: status.as_u16(),
            });
        }

        let nodes = response.json::<Vec<ProviderNode>>().await.map_err(|e| {
            AppError::InvalidProviderPayload(format!(
                "response is not a valid node ranking payload: {e}"
            ))
        })?;

        Ok(nodes)
    }
}
