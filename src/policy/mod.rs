pub mod macros;

use sea_orm::{QueryFilter, Select};

/// Policy trait for authorization logic.
///
/// Implement this trait for each entity type that requires authorization.
/// The policy provides methods to check permissions and filter queries.
///
/// # Type Parameters
/// * `E` - The entity type this policy authorizes
///
/// # Example
/// ```rust,ignore
/// use api_core::auth::CurrentUser;
/// use api_core::policy::Policy;
/// use crate::database::models::{average, user};
///
/// pub struct AveragePolicy {
///     current_user: Option<CurrentUser<user::Model>>,
/// }
///
/// impl Policy<average::Entity> for AveragePolicy {
///     fn can_read(&self, average: &average::Model) -> bool {
///         match &self.current_user {
///             Some(user) => average.user_id == user.id,
///             None => false,
///         }
///     }
///
///     fn readable(&self, query: Select<average::Entity>) -> Select<average::Entity> {
///         match &self.current_user {
///             Some(user) => query.filter(average::Column::UserId.eq(user.id)),
///             None => query.limit(0), // Return no results for unauthenticated users
///         }
///     }
/// }
/// ```
pub trait Policy<E>
where
    E: sea_orm::EntityTrait,
{
    /// Check if the current user can read the given entity.
    ///
    /// # Arguments
    /// * `entity` - The entity model to check read permission for
    ///
    /// # Returns
    /// `true` if the user can read the entity, `false` otherwise
    fn can_read(&self, entity: &E::Model) -> bool;

    /// Filter a query to only return entities the current user can read.
    ///
    /// This method takes a query and applies filters to limit results to
    /// entities the current user has permission to access. This is Ruby on Rails
    /// policy.scope-style authorization.
    ///
    /// # Arguments
    /// * `query` - The base query to filter
    ///
    /// # Returns
    /// The filtered query with authorization conditions applied
    fn readable(&self, query: Select<E>) -> Select<E>;

    /// Check if the current user can create an entity of this type.
    ///
    /// # Returns
    /// `true` if the user can create entities, `false` otherwise
    fn can_create(&self) -> bool {
        false
    }

    /// Check if the current user can update the given entity.
    ///
    /// By default, delegates to `can_read`. Override for different update permissions.
    ///
    /// # Arguments
    /// * `entity` - The entity model to check update permission for
    ///
    /// # Returns
    /// `true` if the user can update the entity, `false` otherwise
    fn can_update(&self, entity: &E::Model) -> bool {
        self.can_read(entity)
    }

    /// Check if the current user can delete the given entity.
    ///
    /// By default, delegates to `can_update`. Override for different delete permissions.
    ///
    /// # Arguments
    /// * `entity` - The entity model to check delete permission for
    ///
    /// # Returns
    /// `true` if the user can delete the entity, `false` otherwise
    fn can_delete(&self, entity: &E::Model) -> bool {
        self.can_update(entity)
    }

    /// Check if the current user can view the entity in a specific view.
    ///
    /// By default, uses the same logic as `can_read`. Override this method
    /// if different views require different permission levels (e.g., "detailed"
    /// view requires admin access).
    ///
    /// # Arguments
    /// * `entity` - The entity model to check view permission for
    /// * `view_name` - The name of the view being requested
    ///
    /// # Returns
    /// `true` if the user can view the entity in the specified view, `false` otherwise
    fn can_view(&self, entity: &E::Model, _view_name: &str) -> bool {
        self.can_read(entity)
    }
}
