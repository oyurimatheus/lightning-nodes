use crate::features::nodes::queries::repository::{NodePage, NodeReader};

pub struct ListNodes {
    reader: std::sync::Arc<dyn NodeReader>,
}

impl ListNodes {
    pub fn new(reader: std::sync::Arc<dyn NodeReader>) -> Self {
        Self { reader }
    }

    pub async fn execute(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<NodePage, crate::shared::error::AppError> {
        self.reader.list_active_nodes(limit, offset).await
    }
}
