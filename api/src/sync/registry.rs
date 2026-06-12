use std::collections::HashMap;

use futures_util::future::BoxFuture;
use sea_orm::{DatabaseConnection, PrimaryKeyTrait};
use uuid::Uuid;

use crate::{
    database::models::user,
    policy::Policy,
    share::principal::{FromPrincipal, Principal},
    sync::from_user::FromUser,
    sync::syncable::Syncable,
};

trait SyncHandler: Send + Sync {
    fn can_principal_read(&self, snapshot: &serde_json::Value, principal: &Principal) -> bool;

    /// Whether this entity type was registered as shareable.
    fn shareable(&self) -> bool {
        false
    }

    /// Whether `user` may create shares for the entity with this ID.
    /// Only shareable registrations implement this; others always deny.
    fn can_user_share<'a>(
        &'a self,
        _db: &'a DatabaseConnection,
        _entity_id: Uuid,
        _user: &'a user::Model,
    ) -> BoxFuture<'a, bool> {
        Box::pin(std::future::ready(false))
    }
}

/// User-scoped registration: the policy is built via `FromUser` from the
/// principal's user. Anonymous principals and share tokens never match.
struct EntitySyncHandler<E>(std::marker::PhantomData<E>);

impl<E> SyncHandler for EntitySyncHandler<E>
where
    E: Syncable,
    E::Policy: FromUser,
    E::Model: serde::de::DeserializeOwned,
{
    fn can_principal_read(&self, snapshot: &serde_json::Value, principal: &Principal) -> bool {
        let Some(user) = &principal.user else {
            return false;
        };
        match serde_json::from_value::<E::Model>(snapshot.clone()) {
            Ok(entity) => E::Policy::from_user(user).can_read(&entity),
            Err(_) => false,
        }
    }
}

/// Share-aware registration: the policy is built via `FromPrincipal`, so the
/// principal's active shares widen read access alongside user ownership.
struct ShareableEntitySyncHandler<E>(std::marker::PhantomData<E>);

impl<E> SyncHandler for ShareableEntitySyncHandler<E>
where
    E: Syncable,
    E::Policy: FromPrincipal,
    E::Model: serde::de::DeserializeOwned,
    <E::PrimaryKey as PrimaryKeyTrait>::ValueType: From<Uuid>,
{
    fn can_principal_read(&self, snapshot: &serde_json::Value, principal: &Principal) -> bool {
        match serde_json::from_value::<E::Model>(snapshot.clone()) {
            Ok(entity) => E::Policy::from_principal(principal).can_read(&entity),
            Err(_) => false,
        }
    }

    fn shareable(&self) -> bool {
        true
    }

    fn can_user_share<'a>(
        &'a self,
        db: &'a DatabaseConnection,
        entity_id: Uuid,
        user: &'a user::Model,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let Ok(Some(entity)) = E::find_by_id(entity_id).one(db).await else {
                return false;
            };
            // Sharing an entity requires the same authority as updating it.
            let principal = Principal::from_user_model(user.clone());
            E::Policy::from_principal(&principal).can_update(&entity)
        })
    }
}

/// Maps entity type names to type-erased policy-based read checkers.
///
/// Built at boot time by calling `register::<E>()` (user-scoped) or
/// `register_shareable::<E>()` (share-aware) for each syncable entity.
/// The sync listener uses this registry to determine which connected
/// principals should receive push events for each change, and the share
/// handlers use it to authorize share creation.
pub struct SyncRegistry {
    handlers: HashMap<&'static str, Box<dyn SyncHandler>>,
}

impl Default for SyncRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register an entity type. The entity's associated `Policy` will be used
    /// (via `FromUser`) to evaluate read access for each connected user when
    /// a change event arrives. Share tokens never grant access to entities
    /// registered this way.
    pub fn register<E>(mut self) -> Self
    where
        E: Syncable,
        E::Policy: FromUser,
        E::Model: serde::de::DeserializeOwned,
    {
        self.handlers.insert(
            E::entity_type(),
            Box::new(EntitySyncHandler::<E>(std::marker::PhantomData)),
        );
        self
    }

    /// Register a shareable entity type. The entity's associated `Policy` is
    /// built via `FromPrincipal`, so active shares held by a connection widen
    /// push delivery, and shares may be created for this entity type.
    pub fn register_shareable<E>(mut self) -> Self
    where
        E: Syncable,
        E::Policy: FromPrincipal,
        E::Model: serde::de::DeserializeOwned,
        <E::PrimaryKey as PrimaryKeyTrait>::ValueType: From<Uuid>,
    {
        self.handlers.insert(
            E::entity_type(),
            Box::new(ShareableEntitySyncHandler::<E>(std::marker::PhantomData)),
        );
        self
    }

    /// Returns `true` if the given principal can read the entity described by
    /// `snapshot`.
    ///
    /// Falls back to `false` for entity types not registered in this registry,
    /// preventing accidental data leaks for unregistered entities.
    pub fn can_principal_read(
        &self,
        entity_type: &str,
        snapshot: &serde_json::Value,
        principal: &Principal,
    ) -> bool {
        match self.handlers.get(entity_type) {
            Some(handler) => handler.can_principal_read(snapshot, principal),
            None => false,
        }
    }

    /// Returns `true` if the given user can read the entity described by `snapshot`.
    ///
    /// Convenience wrapper around [`Self::can_principal_read`] for a plain
    /// authenticated user with no shares.
    pub fn can_user_read(
        &self,
        entity_type: &str,
        snapshot: &serde_json::Value,
        user: &user::Model,
    ) -> bool {
        let principal = Principal::from_user_model(user.clone());
        self.can_principal_read(entity_type, snapshot, &principal)
    }

    /// Whether `entity_type` was registered with [`Self::register_shareable`].
    pub fn is_shareable(&self, entity_type: &str) -> bool {
        self.handlers
            .get(entity_type)
            .is_some_and(|h| h.shareable())
    }

    /// Whether `user` may create shares for the given entity.
    ///
    /// Loads the entity and checks `can_update` on its policy — sharing
    /// requires the same authority as updating. Returns `false` for entity
    /// types not registered as shareable and for missing entities.
    pub async fn can_user_share(
        &self,
        db: &DatabaseConnection,
        entity_type: &str,
        entity_id: Uuid,
        user: &user::Model,
    ) -> bool {
        match self.handlers.get(entity_type) {
            Some(handler) => handler.can_user_share(db, entity_id, user).await,
            None => false,
        }
    }
}
