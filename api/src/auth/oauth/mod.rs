//! Social OAuth login (Google, Discord, Apple).
//!
//! Flow:
//! 1. `GET /api/auth/oauth/{provider}/start` → 302 to provider
//! 2. Provider redirects to `GET /api/auth/oauth/{provider}/callback`
//! 3. API creates/links user, stores one-time exchange token, redirects to
//!    `{app_url}/oauth/callback?code=...`
//! 4. App `POST /api/auth/oauth/exchange` with the code → JWT pair

mod handlers;
mod providers;
mod state;
mod upsert;

pub use handlers::{oauth_callback, oauth_exchange, oauth_providers, oauth_start};
pub use providers::OauthProvider;
