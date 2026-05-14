use chrono::Utc;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};

use crate::{app::App, jobs::{Job, JobError}};

pub struct ExpireSubscriptionsJob<ExtraConfig = ()>(std::marker::PhantomData<ExtraConfig>);

#[derive(Debug, Serialize, Deserialize)]
pub struct ExpireSubscriptionsArgs {}

impl<ExtraConfig: Clone + Send + Sync + 'static> Job<ExtraConfig>
    for ExpireSubscriptionsJob<ExtraConfig>
{
    type Arguments = ExpireSubscriptionsArgs;

    fn name() -> &'static str {
        "expire_subscriptions"
    }

    async fn execute(app: &App<ExtraConfig>, _args: Self::Arguments) -> Result<(), JobError> {
        expire_trial_and_gift_subscriptions(&app.db)
            .await
            .map_err(|e| JobError::TryAgainLater(e.to_string()))
    }
}

async fn expire_trial_and_gift_subscriptions(
    db: &DatabaseConnection,
) -> Result<(), sea_orm::DbErr> {
    use crate::billing::models::{gift_subscription, trial_subscription};
    use crate::database::models::user;

    let now = Utc::now().naive_utc();

    // Find users with expired trial subscriptions
    let expired_trials = trial_subscription::Entity::find()
        .filter(trial_subscription::Column::ActiveUntil.lt(now))
        .all(db)
        .await?;

    for trial in expired_trials {
        let u = user::Entity::find_by_id(trial.user_id).one(db).await?;
        if let Some(u) = u {
            // Only clear cache if this trial is the currently cached subscription
            if u.subscription_id == Some(trial.id) {
                user::Entity::update(user::ActiveModel {
                    id: Set(u.id),
                    subscription_id: Set(None),
                    subscription_type: Set(None),
                    subscription_plan: Set(None),
                    ..Default::default()
                })
                .exec(db)
                .await?;
            }
        }
    }

    // Find users with expired gift subscriptions
    let expired_gifts = gift_subscription::Entity::find()
        .filter(gift_subscription::Column::ActiveUntil.lt(now))
        .all(db)
        .await?;

    for gift in expired_gifts {
        let u = user::Entity::find_by_id(gift.user_id).one(db).await?;
        if let Some(u) = u {
            if u.subscription_id == Some(gift.id) {
                user::Entity::update(user::ActiveModel {
                    id: Set(u.id),
                    subscription_id: Set(None),
                    subscription_type: Set(None),
                    subscription_plan: Set(None),
                    ..Default::default()
                })
                .exec(db)
                .await?;
            }
        }
    }

    Ok(())
}
