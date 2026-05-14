use async_trait::async_trait;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

#[async_trait]
pub trait MetricsCollector: Send + Sync {
    async fn collect(&self, db: &DatabaseConnection);
}

#[derive(Default, Clone)]
pub struct CollectorRegistry(Vec<Arc<dyn MetricsCollector>>);

impl CollectorRegistry {
    pub fn add<C: MetricsCollector + 'static>(&mut self, collector: C) {
        self.0.push(Arc::new(collector));
    }

    pub async fn collect_all(&self, db: &DatabaseConnection) {
        for collector in &self.0 {
            collector.collect(db).await;
        }
    }
}
