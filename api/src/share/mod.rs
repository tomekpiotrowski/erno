//! Docs: docs/src/content/docs/api/share.md
pub mod extractor;
pub mod models;
pub mod principal;

pub use extractor::SHARE_TOKEN_HEADER;
pub use principal::{resolve_principal, resolve_share_token, ActiveShare, FromPrincipal, Principal};
