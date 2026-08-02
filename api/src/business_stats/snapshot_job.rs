use serde::{Deserialize, Serialize};

use crate::{
    app::App,
    jobs::{Job, JobError},
};

use super::metrics::compute_and_store_snapshot;

pub struct BusinessStatsSnapshotJob<ExtraConfig = ()>(std::marker::PhantomData<ExtraConfig>);

#[derive(Debug, Serialize, Deserialize)]
pub struct BusinessStatsSnapshotArgs {}

impl<ExtraConfig: Clone + Send + Sync + 'static> Job<ExtraConfig>
    for BusinessStatsSnapshotJob<ExtraConfig>
{
    type Arguments = BusinessStatsSnapshotArgs;

    fn name() -> &'static str {
        "business_stats_snapshot"
    }

    async fn execute(app: &App<ExtraConfig>, _args: Self::Arguments) -> Result<(), JobError> {
        compute_and_store_snapshot(&app.db)
            .await
            .map_err(|e| JobError::TryAgainLater(e.to_string()))
    }
}
