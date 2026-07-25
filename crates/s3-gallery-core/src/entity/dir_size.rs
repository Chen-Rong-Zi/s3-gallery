use sea_orm::entity::prelude::*;
use crate::types::{HostId, Prefix};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "dir_sizes")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,
    #[sea_orm(primary_key)]
    pub dir_path: Prefix,
    pub total_size: i64,
    pub total_files: i64,
}

impl ActiveModelBehavior for ActiveModel {}