use chrono::{DateTime, Utc};
use std::collections::HashSet;
use uuid::Uuid;

use crate::shared::error::AppError;

use super::node::LightningNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotStatus {
    Loading,
    Ready,
    Failed,
}

impl SnapshotStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SnapshotStatus::Loading => "loading",
            SnapshotStatus::Ready => "ready",
            SnapshotStatus::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: Uuid,
    pub status: SnapshotStatus,
    pub created_at: DateTime<Utc>,

    pub saved_at: Option<DateTime<Utc>>,
    pub node_count: i32,
    pub error_message: Option<String>,
}

pub fn validate_unique_public_keys(nodes: &[LightningNode]) -> Result<(), AppError> {
    let mut seen = HashSet::with_capacity(nodes.len());
    for node in nodes {
        if !seen.insert(node.public_key.as_str()) {
            return Err(AppError::DuplicateNode(node.public_key.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(key: &str) -> LightningNode {
        LightningNode::new(key, "alias", 1, 0).unwrap()
    }

    #[test]
    fn unique_keys_pass() {
        assert!(validate_unique_public_keys(&[node("a"), node("b")]).is_ok());
    }

    #[test]
    fn duplicate_keys_rejected() {
        let err = validate_unique_public_keys(&[node("a"), node("b"), node("a")]).unwrap_err();
        assert!(matches!(err, AppError::DuplicateNode(k) if k == "a"));
    }

    #[test]
    fn empty_set_passes() {
        assert!(validate_unique_public_keys(&[]).is_ok());
    }
}
