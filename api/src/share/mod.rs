//! Docs: docs/src/content/docs/api/share.md
pub mod extractor;
pub mod handlers;
pub mod models;
pub mod principal;
pub mod router;

#[cfg(test)]
mod tests;

pub use extractor::SHARE_TOKEN_HEADER;
pub use principal::{resolve_principal, resolve_share_token, ActiveShare, FromPrincipal, Principal};
pub use router::share_router;
