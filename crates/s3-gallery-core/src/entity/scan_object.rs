use sea_orm::entity::prelude::*;
use crate::types::{HostId, ObjectKey, Etag, FileSize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "scan_objects")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub scan_id: String,
    #[sea_orm(primary_key)]
    pub key: ObjectKey,
    pub host_id: HostId,
    pub etag: Etag,
    pub size: FileSize,
    pub last_modified: String,
    pub is_deleted: bool,
}

impl ActiveModelBehavior for ActiveModel {}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::Key",
        to = "super::file::Column::Key"
    )]
    File,
}