/// Authorize an action on an entity, returning 403 Forbidden if not permitted.
///
/// This macro checks if the current user has permission to perform an action
/// on a given entity. If permission is denied, it returns early with a 403 error.
///
/// # Usage
///
/// ```rust,ignore
/// use api_core::{authorize, policy::Policy};
///
/// // Check read permission
/// authorize!(policy, read, &entity)?;
///
/// // Check create permission
/// authorize!(policy, create)?;
///
/// // Check update permission
/// authorize!(policy, update, &entity)?;
///
/// // Check delete permission
/// authorize!(policy, delete, &entity)?;
/// ```
#[macro_export]
macro_rules! authorize {
    ($policy:expr, read, $entity:expr) => {
        if !$policy.can_read($entity) {
            return Err($crate::api::request_result::RequestError::forbidden());
        }
    };
    ($policy:expr, create) => {
        if !$policy.can_create() {
            return Err($crate::api::request_result::RequestError::forbidden());
        }
    };
    ($policy:expr, update, $entity:expr) => {
        if !$policy.can_update($entity) {
            return Err($crate::api::request_result::RequestError::forbidden());
        }
    };
    ($policy:expr, delete, $entity:expr) => {
        if !$policy.can_delete($entity) {
            return Err($crate::api::request_result::RequestError::forbidden());
        }
    };
}

/// Authorize a view on an entity, returning 403 Forbidden if not permitted.
///
/// This macro checks if the current user has permission to view an entity
/// in a specific view. If permission is denied, it returns early with a 403 error.
///
/// # Usage
///
/// ```rust,ignore
/// use api_core::{authorize_view, policy::Policy};
///
/// authorize_view!(policy, &entity, view)?;
/// ```
#[macro_export]
macro_rules! authorize_view {
    ($policy:expr, $entity:expr, $view:expr) => {
        if !$policy.can_view($entity, $view.name()) {
            return Err($crate::api::request_result::RequestError::forbidden());
        }
    };
}
