use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app::App,
    database::models::{user_token, user_token_type::UserTokenType},
    emails::send_html_email,
    jobs::{Job, JobError},
    token::hash_token,
};

pub struct SendVerificationEmailJob<ExtraConfig = ()>(std::marker::PhantomData<ExtraConfig>);

#[derive(Debug, Serialize, Deserialize)]
pub struct SendVerificationEmailArgs {
    pub user_id: Uuid,
    pub email: String,
    pub raw_token: String,
}

impl<ExtraConfig: Clone + Send + Sync + 'static> Job<ExtraConfig>
    for SendVerificationEmailJob<ExtraConfig>
{
    type Arguments = SendVerificationEmailArgs;

    fn name() -> &'static str {
        "send_verification_email"
    }

    async fn execute(app: &App<ExtraConfig>, args: Self::Arguments) -> Result<(), JobError> {
        user_token::Entity::delete_many()
            .filter(user_token::Column::UserId.eq(args.user_id))
            .filter(user_token::Column::TokenType.eq(UserTokenType::EmailVerification))
            .exec(&app.db)
            .await
            .map_err(|e| JobError::TryAgainLater(e.to_string()))?;

        let expires_at = Utc::now()
            + chrono::Duration::hours(app.config.auth.token_expiration_hours as i64);

        user_token::ActiveModel {
            user_id: Set(args.user_id),
            token_type: Set(UserTokenType::EmailVerification),
            token_hash: Set(hash_token(&args.raw_token)),
            expires_at: Set(expires_at.naive_utc()),
            ..Default::default()
        }
        .insert(&app.db)
        .await
        .map_err(|e| JobError::TryAgainLater(e.to_string()))?;

        let verify_url = format!(
            "{}/verify-email?token={}",
            app.config.base_url, args.raw_token
        );
        let body = format!(
            "<p>Click <a href=\"{url}\">here</a> to verify your email.</p><p>Or paste: {url}</p>",
            url = verify_url
        );

        send_html_email(app, &args.email, "Verify your email", body)
            .await
            .map_err(|e| JobError::TryAgainLater(e.to_string()))
    }
}
