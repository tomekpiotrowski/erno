use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set};

use crate::billing::{
    handlers::webhooks::update_user_subscription_cache,
    models::trial_subscription,
};

/// Create a trial subscription for a user if they don't already have one.
///
/// Silently no-ops if a trial already exists for this user.
/// Updates the user subscription cache on successful insert.
pub async fn create_trial(
    db: &DatabaseConnection,
    user_id: uuid::Uuid,
    plan: impl Into<String>,
    days: u32,
) -> Result<(), DbErr> {
    // Only one trial per user — silently skip if one already exists
    let existing = trial_subscription::Entity::find()
        .filter(trial_subscription::Column::UserId.eq(user_id))
        .one(db)
        .await?;

    if existing.is_some() {
        return Ok(());
    }

    let plan = plan.into();
    let active_until = Utc::now().naive_utc() + chrono::Duration::days(days as i64);
    let now = Utc::now().naive_utc();

    let inserted = trial_subscription::ActiveModel {
        user_id: Set(user_id),
        plan: Set(plan.clone()),
        active_until: Set(active_until),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;

    update_user_subscription_cache(
        db,
        user_id,
        Some(inserted.id),
        Some("trial".to_string()),
        Some(plan),
    )
    .await?;

    Ok(())
}
