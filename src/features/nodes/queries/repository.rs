use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::features::nodes::domain::{
    LightningNode, capacity_to_btc_string, unix_seconds_to_rfc3339,
};
use crate::shared::error::AppError;

/// A node of the active snapshot together with the snapshot's persisted
/// publication timestamp (`saved_at`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveNode {
    pub node: LightningNode,
    pub saved_at: DateTime<Utc>,
}

impl ActiveNode {
    /// Serialize for the HTTP response using integer arithmetic only.
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

/// Read path for `GET /nodes`. Returns nodes of the latest successfully
/// published snapshot; never touches the external provider.
#[async_trait]
pub trait NodeReader: Send + Sync {
    async fn list_active_nodes(&self) -> Result<Vec<ActiveNode>, AppError>;
}
