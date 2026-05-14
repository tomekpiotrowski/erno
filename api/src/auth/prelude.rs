//! Prelude for authentication and authorization.
//!
//! Import this module to bring common auth types and macros into scope.
//!
//! # Example
//! ```rust,ignore
//! use api_core::auth::prelude::*;
//!
//! pub async fn show(
//!     current_user: CurrentUser<user::Model>,
//!     policy: UserPolicy,
//!     view: ViewParam<UserView>,
//! ) -> RequestResult {
//!     authorize!(policy, read, &entity)?;
//!     Ok(Json(view.render(entity)))
//! }
//! ```

// Re-export authentication types
pub use crate::auth::CurrentUser;

// Re-export policy traits
pub use crate::policy::Policy;

// Re-export view types
pub use crate::api::view_param::{Renderer, ViewEnum, ViewParam};

// Re-export authorization macros
pub use crate::{authorize, authorize_view};
