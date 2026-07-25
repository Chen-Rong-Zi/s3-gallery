use sea_orm::entity::prelude::*;
use crate::types::{ObjectKey, MetadataNamespace};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "metadata")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub file_key: ObjectKey,
    #[sea_orm(primary_key)]
    pub namespace: MetadataNamespace,
    #[sea_orm(primary_key)]
    pub key: String,
    pub namespace_custom: Option<String>,
    pub value: String,
    pub extracted_at: String,
    pub partial: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::FileKey",
        to = "super::file::Column::Key"
    )]
    File,
}

impl ActiveModelBehavior for ActiveModel {}