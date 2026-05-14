use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub email_verified_at: Option<NaiveDateTime>,
    pub token_version: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::user_token::Entity")]
    UserToken,
}

impl Related<super::user_token::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UserToken.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
