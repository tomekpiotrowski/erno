//! Optional branded HTML email templates loaded from `email_templates_dir`.

use std::collections::HashMap;
use std::path::Path;

/// Known template file stems under `email_templates_dir`.
#[derive(Debug, Clone, Copy)]
pub enum EmailTemplate {
    Verification,
    PasswordReset,
    AlreadyRegistered,
    /// Operator alert: an error fingerprint was seen for the first time.
    NewIssue,
}

impl EmailTemplate {
    fn file_name(self) -> &'static str {
        match self {
            Self::Verification => "verification.html",
            Self::PasswordReset => "password_reset.html",
            Self::AlreadyRegistered => "already_registered.html",
            Self::NewIssue => "new_issue.html",
        }
    }
}

/// Load a template from disk and substitute `{{key}}` placeholders.
/// Returns `None` when no templates dir is configured or the file is missing.
pub fn render_template(
    templates_dir: Option<&str>,
    template: EmailTemplate,
    vars: &HashMap<&str, String>,
) -> Option<String> {
    let dir = templates_dir?;
    let path = Path::new(dir).join(template.file_name());
    let mut body = std::fs::read_to_string(&path).ok()?;
    for (key, value) in vars {
        body = body.replace(&format!("{{{{{key}}}}}"), value);
    }
    Some(body)
}

/// Render a template or return the provided plain fallback HTML.
pub fn render_or_fallback(
    templates_dir: Option<&str>,
    template: EmailTemplate,
    vars: &HashMap<&str, String>,
    fallback: String,
) -> String {
    render_template(templates_dir, template, vars).unwrap_or(fallback)
}

/// Common placeholders for auth emails.
pub fn base_vars(email: &str, app_url: &str, expiry_hours: u64) -> HashMap<&'static str, String> {
    let mut map = HashMap::new();
    map.insert("email", email.to_string());
    map.insert("expiry_hours", expiry_hours.to_string());
    map.insert("login_url", format!("{app_url}/login"));
    map
}
