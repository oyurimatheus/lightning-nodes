use crate::features::nodes::queries::repository::{ActiveNode, NodeReader};

/// Read use case: thin wrapper that keeps HTTP handlers free of repository
/// details and gives a single point for future caching (if benchmarks justify
/// it).
pub struct ListNodes {
    reader: std::sync::Arc<dyn NodeReader>,
}

impl ListNodes {
    pub fn new(reader: std::sync::Arc<dyn NodeReader>) -> Self {
        Self { reader }
    }

    pub async fn execute(&self) -> Result<Vec<ActiveNode>, crate::shared::error::AppError> {
        self.reader.list_active_nodes().await
    }
}
