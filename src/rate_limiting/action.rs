/// Identifies a specific action for rate limiting purposes.
///
/// Used to apply different rate limits to different endpoints. For example,
/// user registration might have a stricter limit than general API calls.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RateLimitAction(pub String);

impl RateLimitAction {
    /// Create a new rate limit action identifier
    pub fn new(action: impl Into<String>) -> Self {
        Self(action.into())
    }

    /// Get the action name as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
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
