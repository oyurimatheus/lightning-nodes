use serde::Serialize;

#[derive(Debug, Serialize, PartialEq)]
pub struct NodeResponse {
    pub public_key: String,
    pub alias: String,

    pub capacity: String,

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
