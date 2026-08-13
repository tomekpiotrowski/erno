//! Operator event log + matching Prometheus counters.
//!
//! Docs: docs/src/content/docs/api/console.md

use sea_orm::{ActiveModelTrait, ConnectionTrait, DbErr, Set};
use serde_json::{json, Value as JsonValue};
use uuid::Uuid;

use crate::database::models::admin_event;

pub const USER_REGISTERED: &str = "user.registered";
pub const USER_VERIFIED: &str = "user.verified";
pub const USER_DELETED: &str = "user.deleted";
pub const SUBSCRIPTION_ACTIVATED: &str = "subscription.activated";
pub const SUBSCRIPTION_CANCELED: &str = "subscription.canceled";
pub const SUBSCRIPTION_GIFTED: &str = "subscription.gifted";

/// Insert an `admin_event` row and increment the matching counter.
///
/// Logging a failure must not fail the originating request: callers that cannot
/// surface `DbErr` should use [`emit_ok`].
pub async fn emit<C: ConnectionTrait>(
    db: &C,
    name: &str,
    user_id: Option<Uuid>,
    payload: JsonValue,
) -> Result<(), DbErr> {
    increment_counter(name);

    admin_event::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(name.to_string()),
        user_id: Set(user_id),
        payload: Set(payload),
        created_at: Set(chrono::Utc::now().naive_utc()),
    }
    .insert(db)
    .await?;

    Ok(())
}

/// Like [`emit`], but logs and swallows persistence errors.
pub async fn emit_ok<C: ConnectionTrait>(
    db: &C,
    name: &str,
    user_id: Option<Uuid>,
    payload: JsonValue,
) {
    if let Err(e) = emit(db, name, user_id, payload).await {
        tracing::error!(name, error = %e, "failed to persist admin_event");
    }
}

pub fn empty_payload() -> JsonValue {
    json!({})
}

fn increment_counter(name: &str) {
    let metric = match name {
        USER_REGISTERED => "erno_users_registered_total",
        USER_VERIFIED => "erno_users_verified_total",
        USER_DELETED => "erno_users_deleted_total",
        SUBSCRIPTION_ACTIVATED => "erno_subscriptions_activated_total",
        SUBSCRIPTION_CANCELED => "erno_subscriptions_canceled_total",
        SUBSCRIPTION_GIFTED => "erno_subscriptions_gifted_total",
        _ => return,
    };
    metrics::counter!(metric).increment(1);
}
