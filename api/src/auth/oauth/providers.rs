use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OauthProvider {
    Google,
    Discord,
    Apple,
}

impl OauthProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Discord => "discord",
            Self::Apple => "apple",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Google, Self::Discord, Self::Apple]
    }
}

impl fmt::Display for OauthProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OauthProvider {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "google" => Ok(Self::Google),
            "discord" => Ok(Self::Discord),
            "apple" => Ok(Self::Apple),
            _ => Err(()),
        }
    }
}

/// Normalized profile returned by a provider after code exchange.
#[derive(Debug, Clone)]
pub struct OauthProfile {
    pub subject: String,
    pub email: String,
    /// When true (or unknown-but-provider-trusted), we mark email verified.
    pub email_verified: bool,
    pub display_name: Option<String>,
}
