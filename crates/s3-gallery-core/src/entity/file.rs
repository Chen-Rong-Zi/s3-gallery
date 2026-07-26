use crate::types::{Etag, FileSize, FileType, HostId, MetadataState, ObjectKey};
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "files")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,
    #[sea_orm(primary_key)]
    pub key: ObjectKey,
    pub etag: Etag,
    pub size: FileSize,
    pub last_modified: String,
    pub content_type: Option<String>,
    pub file_type: FileType,
    pub metadata_state: MetadataState,
    pub is_deleted: bool,
    pub effective_date: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::host_config::Entity",
        from = "Column::HostId",
        to = "super::host_config::Column::HostId"
    )]
    HostConfig,
}

impl ActiveModelBehavior for ActiveModel {}
