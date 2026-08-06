use serde::{Deserialize, Serialize};

use crate::{
    app::App,
    emails::send_html_email,
    jobs::{Job, JobError},
};

pub struct SendAlreadyRegisteredEmailJob<ExtraConfig = ()>(std::marker::PhantomData<ExtraConfig>);

#[derive(Debug, Serialize, Deserialize)]
pub struct SendAlreadyRegisteredEmailArgs {
    pub email: String,
}

impl<ExtraConfig: Clone + Send + Sync + 'static> Job<ExtraConfig>
    for SendAlreadyRegisteredEmailJob<ExtraConfig>
{
    type Arguments = SendAlreadyRegisteredEmailArgs;

    fn name() -> &'static str {
        "send_already_registered_email"
    }

    async fn execute(app: &App<ExtraConfig>, args: Self::Arguments) -> Result<(), JobError> {
        let login_url = format!("{}/login", app.config.app_url());
        let fallback = format!(
            "<p>Someone (possibly you) tried to register an account with this email address, \
             but an account already exists.</p>\
             <p>If this was you, <a href=\"{url}\">log in here</a> instead.</p>\
             <p>If this wasn't you, you can safely ignore this email.</p>",
            url = login_url
        );
        let vars = crate::email_templates::base_vars(
            &args.email,
            app.config.app_url(),
            app.config.auth.one_time_token_expiry_hours,
        );
        let body = crate::email_templates::render_or_fallback(
            app.config.email_templates_dir.as_deref(),
            crate::email_templates::EmailTemplate::AlreadyRegistered,
            &vars,
            fallback,
        );

        send_html_email(
            app,
            &args.email,
            "Someone tried to register your account",
            body,
        )
        .await
        .map_err(|e| JobError::TryAgainLater(e.to_string()))
    }
}
