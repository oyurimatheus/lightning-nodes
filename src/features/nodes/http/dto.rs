use serde::Serialize;

/// Response body of `GET /nodes`.
///
/// In extended mode (default) all five fields are present, including
/// `saved_at`. In compatibility mode (`API_INCLUDE_SAVED_AT=false`) the
/// `saved_at` field is omitted entirely so the payload matches the original
/// challenge contract of exactly four fields.
#[derive(Debug, Serialize, PartialEq)]
pub struct NodeResponse {
    pub public_key: String,
    pub alias: String,
    /// BTC rendered from satoshis with exactly 8 fractional digits (string).
    pub capacity: String,
    /// RFC 3339 UTC with second precision and `Z` suffix.
    pub first_seen: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<String>,
}

impl NodeResponse {
    pub fn from_active(
        node: &crate::features::nodes::queries::repository::ActiveNode,
        include_saved_at: bool,
    ) -> Result<Self, crate::shared::error::AppError> {
        Ok(Self {
            public_key: node.node.public_key.clone(),
            alias: node.node.alias.clone(),
            capacity: node.capacity_btc(),
            first_seen: node.first_seen_rfc3339()?,
            saved_at: if include_saved_at {
                Some(node.saved_at_rfc3339())
            } else {
                None
            },
        })
    }
}
