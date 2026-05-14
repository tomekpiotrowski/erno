use sea_orm::{DatabaseConnection, EntityTrait};

use crate::{
    billing::models::{gift_subscription, stripe_subscription, trial_subscription},
    database::models::user,
};

pub enum CurrentSubscription {
    Stripe(stripe_subscription::Model),
    Gift(gift_subscription::Model),
    Trial(trial_subscription::Model),
}

/// Load the full subscription record for a user using the cached pointer on their user row.
///
/// Uses `user.subscription_id` + `user.subscription_type` to do a single PK lookup on
/// the correct table. Returns `None` if the user has no cached subscription.
pub async fn load_current_subscription(
    db: &DatabaseConnection,
    user: &user::Model,
) -> Option<CurrentSubscription> {
    let sub_id = user.subscription_id?;

    match user.subscription_type.as_deref()? {
        "stripe" => stripe_subscription::Entity::find_by_id(sub_id)
            .one(db)
            .await
            .ok()
            .flatten()
            .map(CurrentSubscription::Stripe),
        "gift" => gift_subscription::Entity::find_by_id(sub_id)
            .one(db)
            .await
            .ok()
            .flatten()
            .map(CurrentSubscription::Gift),
        "trial" => trial_subscription::Entity::find_by_id(sub_id)
            .one(db)
            .await
            .ok()
            .flatten()
            .map(CurrentSubscription::Trial),
        _ => None,
    }
}
