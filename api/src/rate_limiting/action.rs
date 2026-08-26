/// Identifies a specific action for rate limiting purposes.
///
/// Used to apply different rate limits to different endpoints. For example,
/// user registration might have a stricter limit than general API calls.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RateLimitAction(pub String);

impl RateLimitAction {
    /// nginx `auth_request` for Tempo/Loki OTLP ingest (`GET /api/otlp/auth`).
    ///
    /// Exempt from IP quotas: every application replica shares the console
    /// pod's address, so a per-IP limit would be a global ingest ceiling.
    /// The Bearer server token is the control.
    pub const OTLP_AUTH: &'static str = "otlp_auth";

    /// Create a new rate limit action identifier
    pub fn new(action: impl Into<String>) -> Self {
        Self(action.into())
    }

    /// Get the action name as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the limiter should skip this action entirely.
    #[must_use]
    pub fn is_exempt(&self) -> bool {
        self.0 == Self::OTLP_AUTH
    }
}

impl From<&str> for RateLimitAction {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for RateLimitAction {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[cfg(test)]
mod tests {
    use super::RateLimitAction;

    #[test]
    fn only_otlp_auth_is_exempt() {
        assert!(RateLimitAction::new(RateLimitAction::OTLP_AUTH).is_exempt());
        assert!(!RateLimitAction::new("default").is_exempt());
        assert!(!RateLimitAction::new("error_ingest").is_exempt());
    }
}
