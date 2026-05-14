use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "files")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub key: String,
    pub filename: String,
    pub content_type: Option<String>,
    pub byte_size: i64,
    pub checksum: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::file_attachment::Entity")]
    FileAttachments,
}

impl Related<super::file_attachment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::FileAttachments.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
