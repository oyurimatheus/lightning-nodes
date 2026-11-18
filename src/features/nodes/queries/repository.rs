use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::features::nodes::domain::{
    LightningNode, capacity_to_btc_string, unix_seconds_to_rfc3339,
};
use crate::shared::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveNode {
    pub node: LightningNode,
    pub saved_at: DateTime<Utc>,
}

impl ActiveNode {
    pub fn capacity_btc(&self) -> String {
        capacity_to_btc_string(self.node.capacity_sats)
    }

    pub fn first_seen_rfc3339(&self) -> Result<String, AppError> {
        unix_seconds_to_rfc3339(self.node.first_seen.timestamp())
    }

    pub fn saved_at_rfc3339(&self) -> String {
        self.saved_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePage {
    pub nodes: Vec<ActiveNode>,
    pub total_count: u64,
}

#[async_trait]
pub trait NodeReader: Send + Sync {
    async fn list_active_nodes(&self, limit: i64, offset: i64) -> Result<NodePage, AppError>;
}
