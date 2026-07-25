use sea_orm::entity::prelude::*;
use crate::types::TagType;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tags")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub tag_id: i64,
    pub tag_name: String,
    pub tag_type: TagType,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}