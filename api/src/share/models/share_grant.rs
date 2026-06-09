use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// An owner-issued grant of a share to a specific user.
///
/// A grant is effective the moment the row exists — there is no acceptance
/// step; the recipient is only notified. `revoked_at` revokes this single
/// grant without affecting the share's link token or other grants.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "share_grants")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub share_id: Uuid,
    pub user_id: Uuid,
    pub notified_at: Option<NaiveDateTime>,
    pub revoked_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::share::Entity",
        from = "Column::ShareId",
        to = "super::share::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Share,
    #[sea_orm(
        belongs_to = "crate::database::models::user::Entity",
        from = "Column::UserId",
        to = "crate::database::models::user::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    User,
}

impl Related<super::share::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Share.def()
    }
}

impl Related<crate::database::models::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
