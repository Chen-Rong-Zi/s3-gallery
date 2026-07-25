use sea_orm::entity::prelude::*;
use crate::types::HostId;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "scan_metadata")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,
    pub last_scanned_key: Option<String>,
    pub last_scanned_at: Option<String>,
    pub total_files: Option<i64>,
    pub total_size: Option<i64>,
    pub db_schema_version: i64,
}

impl ActiveModelBehavior for ActiveModel {}