use crate::types::ObjectKey;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "file_tags")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub file_key: ObjectKey,
    #[sea_orm(primary_key)]
    pub tag_id: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::tag::Entity",
        from = "Column::TagId",
        to = "super::tag::Column::TagId"
    )]
    Tag,
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::FileKey",
        to = "super::file::Column::Key"
    )]
    File,
}

impl ActiveModelBehavior for ActiveModel {}
