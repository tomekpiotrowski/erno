use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// What a share allows the holder to do with the shared entity.
///
/// v1 only ever issues `Read`; `Write` is reserved so write-capable shares
/// can be added later without a schema migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, DeriveActiveEnum, Serialize, Deserialize, EnumIter)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
#[serde(rename_all = "lowercase")]
pub enum SharePermission {
    #[sea_orm(string_value = "read")]
    Read,
    #[sea_orm(string_value = "write")]
    Write,
}

/// A share of a single entity, created by its owner.
///
/// Access is granted either via a secret link token (`token_hash` is the
/// SHA-256 of the raw token; the raw token is never stored) or via direct
/// `share_grants` rows issued to specific users — or both. `token_hash` is
/// `NULL` for a pure direct grant.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "shares")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[serde(skip_serializing)]
    pub token_hash: Option<String>,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub owner_id: Uuid,
    pub permission: SharePermission,
    pub expires_at: Option<NaiveDateTime>,
    pub revoked_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl Model {
    /// A share is active when it has not been revoked and has not expired.
    pub fn is_active(&self, now: NaiveDateTime) -> bool {
        self.revoked_at.is_none() && self.expires_at.is_none_or(|exp| exp > now)
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::database::models::user::Entity",
        from = "Column::OwnerId",
        to = "crate::database::models::user::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Owner,
    #[sea_orm(has_many = "super::share_grant::Entity")]
    ShareGrant,
}

impl Related<crate::database::models::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Owner.def()
    }
}

impl Related<super::share_grant::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ShareGrant.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
