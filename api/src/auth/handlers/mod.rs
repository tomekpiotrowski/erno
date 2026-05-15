pub mod login;
pub mod logout;
pub mod password_reset;
pub mod refresh;
pub mod register;
pub mod resend_verification;
pub mod verify_email;

use chrono::Utc;
use sea_orm::ActiveModelTrait;
use sea_orm::Set;

use crate::{
    app::App,
    auth::jwt::generate_token,
    database::models::{user, user_token, user_token_type::UserTokenType},
    token::{generate_secure_token, hash_token},
};

#[derive(Debug, serde::Serialize)]
pub struct UserInfo {
    pub id: uuid::Uuid,
    pub email: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserInfo,
}

/// Generate an access JWT and a fresh refresh token, persisting the refresh token to the DB.
pub async fn issue_token_pair<ExtraConfig>(
    app: &App<ExtraConfig>,
    user: &user::Model,
) -> Result<TokenPair, ()>
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let access_token = generate_token(&app.config, user.id, user.token_version).map_err(|_| ())?;

    let raw_refresh = generate_secure_token(64);
    let expires_at = Utc::now().naive_utc()
        + chrono::Duration::days(app.config.auth.refresh_token_days as i64);

    user_token::ActiveModel {
        user_id: Set(user.id),
        token_type: Set(UserTokenType::RefreshToken),
        token_hash: Set(hash_token(&raw_refresh)),
        expires_at: Set(expires_at),
        ..Default::default()
    }
    .insert(&app.db)
    .await
    .map_err(|_| ())?;

    Ok(TokenPair {
        access_token,
        refresh_token: raw_refresh,
        user: UserInfo { id: user.id, email: user.email.clone() },
    })
}
