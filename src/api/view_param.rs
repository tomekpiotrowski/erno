use axum::{
    extract::{FromRequestParts, Query},
    http::request::Parts,
};
use serde::Deserialize;

/// Query parameter extractor for view selection.
///
/// Extracts the `view` query parameter and converts it to the specified view enum.
/// If no view is provided, uses the default view.
///
/// # Type Parameters
/// * `T` - The view enum type implementing `ViewEnum`
///
/// # Example
/// ```rust,ignore
/// use api_core::api::view_param::{ViewParam, ViewEnum, Renderer};
/// use crate::policies::AveragePolicy;
///
/// pub async fn show(
///     policy: AveragePolicy,
///     view: ViewParam<AverageView>,
/// ) -> RequestResult {
///     authorize_view!(policy, &average, view)?;
///     Ok(RequestSuccess::Ok(view.render(average)))
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ViewParam<T> {
    view: T,
}

impl<T> ViewParam<T> {
    /// Get the inner view value.
    pub fn inner(&self) -> &T {
        &self.view
    }

    /// Render an entity using this view.
    pub fn render<E>(&self, entity: E) -> serde_json::Value
    where
        T: Renderer<E>,
    {
        self.view.render(entity)
    }
}

impl<T> std::ops::Deref for ViewParam<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

#[derive(Deserialize)]
struct ViewQuery {
    view: Option<String>,
}

/// Error for invalid view parameter
#[derive(Debug)]
pub struct InvalidViewError;

impl axum::response::IntoResponse for InvalidViewError {
    fn into_response(self) -> axum::response::Response {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid view parameter",
        )
            .into_response()
    }
}

impl<S, T> FromRequestParts<S> for ViewParam<T>
where
    S: Send + Sync,
    T: ViewEnum + Send,
{
    type Rejection = InvalidViewError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(query) = Query::<ViewQuery>::from_request_parts(parts, state)
            .await
            .map_err(|_| InvalidViewError)?;

        let view = match query.view {
            Some(view_name) => T::from_name(&view_name).ok_or(InvalidViewError)?,
            None => T::default_view(),
        };

        Ok(ViewParam { view })
    }
}

/// Trait for view enums that can be used with `ViewParam`.
///
/// Implement this trait for your view enums to enable view-based rendering.
///
/// # Example
/// ```rust,ignore
/// #[derive(Debug, Clone, Copy)]
/// pub enum UserView {
///     Public,
///     Default,
///     Detailed,
/// }
///
/// impl ViewEnum for UserView {
///     fn from_name(name: &str) -> Option<Self> {
///         match name {
///             "public" => Some(Self::Public),
///             "default" => Some(Self::Default),
///             "detailed" => Some(Self::Detailed),
///             _ => None,
///         }
///     }
///
///     fn name(&self) -> &str {
///         match self {
///             Self::Public => "public",
///             Self::Default => "default",
///             Self::Detailed => "detailed",
///         }
///     }
///
///     fn default_view() -> Self {
///         Self::Default
///     }
/// }
/// ```
pub trait ViewEnum: Sized {
    /// Parse a view from its string name.
    ///
    /// # Arguments
    /// * `name` - The view name from the query parameter
    ///
    /// # Returns
    /// `Some(view)` if the name is valid, `None` otherwise
    fn from_name(name: &str) -> Option<Self>;

    /// Get the string name of this view.
    ///
    /// # Returns
    /// The view name as a string slice
    fn name(&self) -> &str;

    /// Get the default view when no view parameter is provided.
    ///
    /// # Returns
    /// The default view variant
    fn default_view() -> Self;
}

/// Trait for rendering entities in different views.
///
/// Implement this trait for your view enums to define how entities
/// should be serialized in each view.
///
/// # Type Parameters
/// * `E` - The entity type being rendered
///
/// # Example
/// ```rust,ignore
/// impl Renderer<user::Model> for UserView {
///     fn render(&self, user: user::Model) -> serde_json::Value {
///         match self {
///             UserView::Public => serde_json::to_value(UserPublicView {
///                 id: user.id,
///                 name: user.name,
///             }).unwrap(),
///             UserView::Default => serde_json::to_value(UserDefaultView {
///                 id: user.id,
///                 name: user.name,
///                 email: user.email,
///                 created_at: user.created_at,
///             }).unwrap(),
///             UserView::Detailed => serde_json::to_value(UserDetailedView::from(user)).unwrap(),
///         }
///     }
/// }
/// ```
pub trait Renderer<E> {
    /// Render an entity as JSON.
    ///
    /// # Arguments
    /// * `entity` - The entity to render
    ///
    /// # Returns
    /// The entity serialized as a JSON value
    fn render(&self, entity: E) -> serde_json::Value;
}
