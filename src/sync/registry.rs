use std::collections::HashMap;

use crate::{
    database::models::user, policy::Policy, sync::from_user::FromUser, sync::syncable::Syncable,
};

trait SyncHandler: Send + Sync {
    fn can_user_read(&self, snapshot: &serde_json::Value, user: &user::Model) -> bool;
}

struct EntitySyncHandler<E>(std::marker::PhantomData<E>);

impl<E> SyncHandler for EntitySyncHandler<E>
where
    E: Syncable,
    E::Model: serde::de::DeserializeOwned,
{
    fn can_user_read(&self, snapshot: &serde_json::Value, user: &user::Model) -> bool {
        match serde_json::from_value::<E::Model>(snapshot.clone()) {
            Ok(entity) => {
                let policy = E::Policy::from_user(user);
                policy.can_read(&entity)
            }
            Err(_) => false,
        }
    }
}

/// Maps entity type names to type-erased policy-based read checkers.
///
/// Built at boot time by calling `register::<E>()` for each syncable entity.
/// The sync listener uses this registry to determine which connected users
/// should receive push events for each change.
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
    /// to evaluate read access for each connected user when a change event arrives.
    pub fn register<E>(mut self) -> Self
    where
        E: Syncable,
        E::Model: serde::de::DeserializeOwned,
    {
        self.handlers.insert(
            E::entity_type(),
            Box::new(EntitySyncHandler::<E>(std::marker::PhantomData)),
        );
        self
    }

    /// Returns `true` if the given user can read the entity described by `snapshot`.
    ///
    /// Falls back to `false` for entity types not registered in this registry,
    /// preventing accidental data leaks for unregistered entities.
    pub fn can_user_read(
        &self,
        entity_type: &str,
        snapshot: &serde_json::Value,
        user: &user::Model,
    ) -> bool {
        match self.handlers.get(entity_type) {
            Some(handler) => handler.can_user_read(snapshot, user),
            None => false,
        }
    }
}
