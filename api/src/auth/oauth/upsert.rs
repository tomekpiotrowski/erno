use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait};

use crate::{
    app::App,
    database::models::{oauth_identity, user},
};

use super::providers::{OauthProfile, OauthProvider};

/// Find or create a user for the OAuth profile, and ensure an oauth_identities row.
pub async fn upsert_oauth_user<ExtraConfig>(
    app: &App<ExtraConfig>,
    provider: OauthProvider,
    profile: &OauthProfile,
) -> Result<user::Model, String>
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let provider_s = provider.as_str().to_string();

    // 1. Existing identity by (provider, subject)
    if let Some(identity) = oauth_identity::Entity::find()
        .filter(oauth_identity::Column::Provider.eq(&provider_s))
        .filter(oauth_identity::Column::ProviderSubject.eq(&profile.subject))
        .one(&app.db)
        .await
        .map_err(|e| e.to_string())?
    {
        let u = user::Entity::find_by_id(identity.user_id)
            .one(&app.db)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "oauth identity user missing".to_string())?;
        return Ok(u);
    }

    let email = profile.email.to_lowercase();
    if email.is_empty() {
        return Err("provider did not return an email".into());
    }
    // Only auto-link / create when the provider asserts a verified email.
    if !profile.email_verified {
        return Err("provider email not verified".into());
    }

    let txn = app.db.begin().await.map_err(|e| e.to_string())?;

    // 2. Link to existing user by email.
    let existing = user::Entity::find()
        .filter(user::Column::Email.eq(&email))
        .one(&txn)
        .await
        .map_err(|e| e.to_string())?;

    let user_row = if let Some(u) = existing {
        if u.email_verified_at.is_none() {
            let mut am: user::ActiveModel = u.into();
            am.email_verified_at = Set(Some(Utc::now().naive_utc()));
            am.update(&txn).await.map_err(|e| e.to_string())?
        } else {
            u
        }
    } else {
        // 3. Create OAuth-only user (no password).
        user::ActiveModel {
            email: Set(email.clone()),
            password_hash: Set(None),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(&txn)
        .await
        .map_err(|e| e.to_string())?
    };

    oauth_identity::ActiveModel {
        user_id: Set(user_row.id),
        provider: Set(provider_s),
        provider_subject: Set(profile.subject.clone()),
        email: Set(Some(email)),
        ..Default::default()
    }
    .insert(&txn)
    .await
    .map_err(|e| e.to_string())?;

    txn.commit().await.map_err(|e| e.to_string())?;
    Ok(user_row)
}
