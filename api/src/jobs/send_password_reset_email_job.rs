use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app::App,
    database::models::{user_token, user_token_type::UserTokenType},
    emails::{send_html_email_with_meta, EmailMeta},
    jobs::{Job, JobError},
    token::hash_token,
};

pub struct SendPasswordResetEmailJob<ExtraConfig = ()>(std::marker::PhantomData<ExtraConfig>);

#[derive(Debug, Serialize, Deserialize)]
pub struct SendPasswordResetEmailArgs {
    pub user_id: Uuid,
    pub email: String,
    pub raw_token: String,
}

impl<ExtraConfig: Clone + Send + Sync + 'static> Job<ExtraConfig>
    for SendPasswordResetEmailJob<ExtraConfig>
{
    type Arguments = SendPasswordResetEmailArgs;

    fn name() -> &'static str {
        "send_password_reset_email"
    }

    async fn execute(app: &App<ExtraConfig>, args: Self::Arguments) -> Result<(), JobError> {
        user_token::Entity::delete_many()
            .filter(user_token::Column::UserId.eq(args.user_id))
            .filter(user_token::Column::TokenType.eq(UserTokenType::PasswordReset))
            .exec(&app.db)
            .await
            .map_err(|e| JobError::TryAgainLater(e.to_string()))?;

        let expires_at = Utc::now()
            + chrono::Duration::hours(app.config.auth.one_time_token_expiry_hours as i64);

        user_token::ActiveModel {
            user_id: Set(args.user_id),
            token_type: Set(UserTokenType::PasswordReset),
            token_hash: Set(hash_token(&args.raw_token)),
            expires_at: Set(expires_at.naive_utc()),
            ..Default::default()
        }
        .insert(&app.db)
        .await
        .map_err(|e| JobError::TryAgainLater(e.to_string()))?;

        let reset_url = format!(
            "{}/reset-password?token={}",
            app.config.app_url(),
            args.raw_token
        );
        let fallback = format!(
            "<p>Click <a href=\"{url}\">here</a> to reset your password.</p><p>Or paste: {url}</p>",
            url = reset_url
        );
        let mut vars = crate::email_templates::base_vars(
            &args.email,
            app.config.app_url(),
            app.config.auth.one_time_token_expiry_hours,
        );
        vars.insert("reset_url", reset_url);
        let body = crate::email_templates::render_or_fallback(
            app.config.email_templates_dir.as_deref(),
            crate::email_templates::EmailTemplate::PasswordReset,
            &vars,
            fallback,
        );

        send_html_email_with_meta(
            app,
            &args.email,
            "Reset your password",
            body,
            EmailMeta {
                template: Some("password_reset".to_string()),
                user_id: Some(args.user_id),
                job_id: None,
            },
        )
        .await
        .map_err(|e| JobError::TryAgainLater(e.to_string()))
    }
}
