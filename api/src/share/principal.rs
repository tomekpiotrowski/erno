use chrono::Utc;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, JoinType, QueryFilter, QuerySelect,
    RelationTrait,
};
use uuid::Uuid;

use crate::database::models::user;
use crate::share::models::share::{self, SharePermission};
use crate::share::models::share_grant;
use crate::token::hash_token;

/// A validated, active share held by a principal.
///
/// Resolved once (from a link token or an account grant) and then carried as
/// pure data, so policy evaluation stays synchronous and DB-free — including
/// on the per-event WebSocket push hot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveShare {
    pub id: Uuid,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub permission: SharePermission,
}

impl From<&share::Model> for ActiveShare {
    fn from(model: &share::Model) -> Self {
        Self {
            id: model.id,
            entity_type: model.entity_type.clone(),
            entity_id: model.entity_id,
            permission: model.permission,
        }
    }
}

/// The unit of authorization: an optional authenticated user plus the set of
/// active shares the request/connection holds.
///
/// A plain authenticated request is `{ user: Some(_), shares: [] }`; an
/// anonymous share-link visitor is `{ user: None, shares: [_] }`.
#[derive(Debug, Clone, Default)]
pub struct Principal {
    pub user: Option<user::Model>,
    pub shares: Vec<ActiveShare>,
}

impl Principal {
    /// Principal for an authenticated user with no shares.
    pub fn from_user_model(user: user::Model) -> Self {
        Self {
            user: Some(user),
            shares: vec![],
        }
    }

    /// IDs of all entities of `entity_type` directly covered by an active share.
    ///
    /// Use this in `Policy::readable` / `can_read` to widen access — both for
    /// the shared entity itself and for implied children (e.g. a comment policy
    /// checks `comment.post_id` against `shared_ids("posts")`).
    pub fn shared_ids(&self, entity_type: &str) -> Vec<Uuid> {
        self.shares
            .iter()
            .filter(|s| s.entity_type == entity_type)
            .map(|s| s.entity_id)
            .collect()
    }

    /// Whether this principal holds an active share for the given share ID.
    pub fn has_share(&self, share_id: Uuid) -> bool {
        self.shares.iter().any(|s| s.id == share_id)
    }
}

/// Trait for constructing a policy instance from a [`Principal`].
///
/// The share-aware sibling of [`crate::sync::from_user::FromUser`]. Implement
/// this alongside `Policy<E>` for entities that can be reached through shares;
/// the policy should widen `readable`/`can_read` with the principal's
/// `shared_ids`. Must stay pure and synchronous — shares are pre-resolved.
pub trait FromPrincipal {
    fn from_principal(principal: &Principal) -> Self;
}

/// Resolve a raw link token to an active share, if one exists.
///
/// Used by the WebSocket `subscribe-share` control message and the principal
/// resolver. The token is looked up by SHA-256 hash; expired or revoked
/// shares resolve to `None`.
pub async fn resolve_share_token(
    db: &DatabaseConnection,
    raw_token: &str,
) -> Result<Option<ActiveShare>, DbErr> {
    let now = Utc::now().naive_utc();
    let found = share::Entity::find()
        .filter(share::Column::TokenHash.eq(hash_token(raw_token)))
        .one(db)
        .await?;

    Ok(found
        .filter(|s| s.is_active(now))
        .map(|s| ActiveShare::from(&s)))
}

/// Resolve a full [`Principal`] from an optional user and raw link tokens.
///
/// This is the single async/DB resolution point:
/// - each raw token is validated (hash lookup, not revoked, not expired);
/// - if a user is present, their active `share_grants` are loaded too, so
///   owner-issued account-bound access works without any token in the request.
pub async fn resolve_principal(
    db: &DatabaseConnection,
    user: Option<user::Model>,
    raw_tokens: &[String],
) -> Result<Principal, DbErr> {
    let now = Utc::now().naive_utc();
    let mut shares: Vec<ActiveShare> = vec![];

    if !raw_tokens.is_empty() {
        let hashes: Vec<String> = raw_tokens.iter().map(|t| hash_token(t)).collect();
        let token_shares = share::Entity::find()
            .filter(share::Column::TokenHash.is_in(hashes))
            .all(db)
            .await?;
        shares.extend(
            token_shares
                .iter()
                .filter(|s| s.is_active(now))
                .map(ActiveShare::from),
        );
    }

    if let Some(user) = &user {
        let granted_shares = share::Entity::find()
            .join(JoinType::InnerJoin, share::Relation::ShareGrant.def())
            .filter(share_grant::Column::UserId.eq(user.id))
            .filter(share_grant::Column::RevokedAt.is_null())
            .all(db)
            .await?;
        shares.extend(
            granted_shares
                .iter()
                .filter(|s| s.is_active(now))
                .map(ActiveShare::from),
        );
    }

    shares.sort_by_key(|s| s.id);
    shares.dedup_by_key(|s| s.id);

    Ok(Principal { user, shares })
}
