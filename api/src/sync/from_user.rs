use crate::database::models::user;

/// Trait for constructing a policy instance from a user model.
///
/// Implement this alongside `Policy<E>` to enable policy-based sync recipient filtering.
pub trait FromUser {
    fn from_user(user: &user::Model) -> Self;
}
