use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use chrono::{DateTime, NaiveDateTime, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use stripe::{Event, EventObject, EventType, Webhook};
use uuid::Uuid;

use crate::{
    app::App,
    billing::models::{
        stripe_subscription,
        subscription_status::SubscriptionStatus,
    },
    database::models::user,
};

pub async fn webhooks<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let stripe_config = match &app.config.stripe {
        Some(c) => c.clone(),
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let sig = match headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
    {
        Some(s) => s.to_owned(),
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    let payload = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let event = match Webhook::construct_event(payload, &sig, &stripe_config.webhook_secret) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Stripe webhook signature validation failed: {}", e);
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    if let Err(e) = handle_event(&app, event).await {
        tracing::error!("Error handling Stripe webhook: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::OK.into_response()
}

async fn handle_event<ExtraConfig: Clone + Send + Sync + 'static>(
    app: &App<ExtraConfig>,
    event: Event,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match event.type_ {
        EventType::CheckoutSessionCompleted => {
            if let EventObject::CheckoutSession(session) = event.data.object {
                handle_checkout_completed(app, session).await?;
            }
        }
        EventType::CustomerSubscriptionUpdated => {
            if let EventObject::Subscription(subscription) = event.data.object {
                handle_subscription_updated(app, subscription).await?;
            }
        }
        EventType::CustomerSubscriptionDeleted => {
            if let EventObject::Subscription(subscription) = event.data.object {
                handle_subscription_deleted(app, subscription).await?;
            }
        }
        EventType::InvoicePaymentSucceeded => {
            if let EventObject::Invoice(invoice) = event.data.object {
                handle_payment_succeeded(app, invoice).await?;
            }
        }
        EventType::InvoicePaymentFailed => {
            if let EventObject::Invoice(invoice) = event.data.object {
                handle_payment_failed(app, invoice).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn handle_checkout_completed<ExtraConfig: Clone + Send + Sync + 'static>(
    app: &App<ExtraConfig>,
    session: stripe::CheckoutSession,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let metadata = session.metadata.unwrap_or_default();
    let user_id_str = metadata.get("user_id").ok_or("missing user_id in metadata")?;
    let plan = metadata.get("plan").ok_or("missing plan in metadata")?.clone();
    let user_id = Uuid::parse_str(user_id_str)?;

    let stripe_subscription_id = match &session.subscription {
        Some(stripe::Expandable::Id(id)) => id.to_string(),
        Some(stripe::Expandable::Object(sub)) => sub.id.to_string(),
        None => return Err("checkout session has no subscription".into()),
    };

    let customer_id = match &session.customer {
        Some(stripe::Expandable::Id(id)) => id.to_string(),
        Some(stripe::Expandable::Object(c)) => c.id.to_string(),
        None => return Err("checkout session has no customer".into()),
    };

    let now = Utc::now().naive_utc();

    let row = stripe_subscription::ActiveModel {
        user_id: Set(user_id),
        stripe_customer_id: Set(customer_id),
        stripe_subscription_id: Set(stripe_subscription_id),
        plan: Set(plan.clone()),
        status: Set(SubscriptionStatus::Active),
        current_period_start: Set(now),
        current_period_end: Set(now),
        cancel_at_period_end: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    let inserted = row.insert(&app.db).await?;

    update_user_subscription_cache(
        &app.db,
        user_id,
        Some(inserted.id),
        Some("stripe".to_string()),
        Some(plan),
    )
    .await?;

    Ok(())
}

async fn handle_subscription_updated<ExtraConfig: Clone + Send + Sync + 'static>(
    app: &App<ExtraConfig>,
    subscription: stripe::Subscription,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let stripe_sub_id = subscription.id.to_string();

    let existing = stripe_subscription::Entity::find()
        .filter(stripe_subscription::Column::StripeSubscriptionId.eq(&stripe_sub_id))
        .one(&app.db)
        .await?;

    let Some(existing) = existing else {
        tracing::warn!(
            "Received subscription.updated for unknown subscription {}",
            stripe_sub_id
        );
        return Ok(());
    };

    let status = stripe_status_to_ours(&subscription.status);
    let period_start =
        timestamp_to_naive(subscription.current_period_start);
    let period_end =
        timestamp_to_naive(subscription.current_period_end);
    let cancel_at_period_end = subscription.cancel_at_period_end;

    let mut active_model: stripe_subscription::ActiveModel = existing.clone().into();
    active_model.status = Set(status.clone());
    active_model.current_period_start = Set(period_start);
    active_model.current_period_end = Set(period_end);
    active_model.cancel_at_period_end = Set(cancel_at_period_end);
    active_model.updated_at = Set(Utc::now().naive_utc());
    active_model.update(&app.db).await?;

    // If the subscription is no longer active, clear the user cache
    if status != SubscriptionStatus::Active {
        let currently_active = user::Entity::find_by_id(existing.user_id)
            .one(&app.db)
            .await?
            .map(|u| u.subscription_id == Some(existing.id))
            .unwrap_or(false);

        if currently_active {
            update_user_subscription_cache(&app.db, existing.user_id, None, None, None).await?;
        }
    }

    Ok(())
}

async fn handle_subscription_deleted<ExtraConfig: Clone + Send + Sync + 'static>(
    app: &App<ExtraConfig>,
    subscription: stripe::Subscription,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let stripe_sub_id = subscription.id.to_string();

    let existing = stripe_subscription::Entity::find()
        .filter(stripe_subscription::Column::StripeSubscriptionId.eq(&stripe_sub_id))
        .one(&app.db)
        .await?;

    let Some(existing) = existing else {
        return Ok(());
    };

    let mut active_model: stripe_subscription::ActiveModel = existing.clone().into();
    active_model.status = Set(SubscriptionStatus::Canceled);
    active_model.updated_at = Set(Utc::now().naive_utc());
    active_model.update(&app.db).await?;

    let currently_active = user::Entity::find_by_id(existing.user_id)
        .one(&app.db)
        .await?
        .map(|u| u.subscription_id == Some(existing.id))
        .unwrap_or(false);

    if currently_active {
        update_user_subscription_cache(&app.db, existing.user_id, None, None, None).await?;
    }

    Ok(())
}

async fn handle_payment_succeeded<ExtraConfig: Clone + Send + Sync + 'static>(
    app: &App<ExtraConfig>,
    invoice: stripe::Invoice,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let stripe_sub_id = match &invoice.subscription {
        Some(stripe::Expandable::Id(id)) => id.to_string(),
        Some(stripe::Expandable::Object(s)) => s.id.to_string(),
        None => return Ok(()),
    };

    let existing = stripe_subscription::Entity::find()
        .filter(stripe_subscription::Column::StripeSubscriptionId.eq(&stripe_sub_id))
        .one(&app.db)
        .await?;

    let Some(existing) = existing else {
        return Ok(());
    };

    if let Some(period_end) = invoice.period_end {
        let new_period_end = timestamp_to_naive(period_end);
        let mut active_model: stripe_subscription::ActiveModel = existing.into();
        active_model.current_period_end = Set(new_period_end);
        active_model.status = Set(SubscriptionStatus::Active);
        active_model.updated_at = Set(Utc::now().naive_utc());
        active_model.update(&app.db).await?;
    }

    Ok(())
}

async fn handle_payment_failed<ExtraConfig: Clone + Send + Sync + 'static>(
    app: &App<ExtraConfig>,
    invoice: stripe::Invoice,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let stripe_sub_id = match &invoice.subscription {
        Some(stripe::Expandable::Id(id)) => id.to_string(),
        Some(stripe::Expandable::Object(s)) => s.id.to_string(),
        None => return Ok(()),
    };

    let existing = stripe_subscription::Entity::find()
        .filter(stripe_subscription::Column::StripeSubscriptionId.eq(&stripe_sub_id))
        .one(&app.db)
        .await?;

    let Some(existing) = existing else {
        return Ok(());
    };

    let mut active_model: stripe_subscription::ActiveModel = existing.clone().into();
    active_model.status = Set(SubscriptionStatus::PastDue);
    active_model.updated_at = Set(Utc::now().naive_utc());
    active_model.update(&app.db).await?;

    let currently_active = user::Entity::find_by_id(existing.user_id)
        .one(&app.db)
        .await?
        .map(|u| u.subscription_id == Some(existing.id))
        .unwrap_or(false);

    if currently_active {
        update_user_subscription_cache(&app.db, existing.user_id, None, None, None).await?;
    }

    Ok(())
}

fn stripe_status_to_ours(status: &stripe::SubscriptionStatus) -> SubscriptionStatus {
    match status {
        stripe::SubscriptionStatus::Active => SubscriptionStatus::Active,
        stripe::SubscriptionStatus::PastDue => SubscriptionStatus::PastDue,
        _ => SubscriptionStatus::Canceled,
    }
}

fn timestamp_to_naive(ts: stripe::Timestamp) -> NaiveDateTime {
    DateTime::from_timestamp(ts, 0)
        .unwrap_or_else(Utc::now)
        .naive_utc()
}

pub async fn update_user_subscription_cache(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    subscription_id: Option<Uuid>,
    subscription_type: Option<String>,
    subscription_plan: Option<String>,
) -> Result<(), sea_orm::DbErr> {
    use sea_orm::ActiveValue::Set;

    let active_user = user::ActiveModel {
        id: Set(user_id),
        subscription_id: Set(subscription_id),
        subscription_type: Set(subscription_type),
        subscription_plan: Set(subscription_plan),
        ..Default::default()
    };

    user::Entity::update(active_user).exec(db).await?;
    Ok(())
}
