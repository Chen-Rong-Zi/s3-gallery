use sea_orm::entity::prelude::*;
use crate::types::{HostId, S3Operation, Direction};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "traffic_log")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub id: i64,
    pub host_id: HostId,
    pub operation: S3Operation,
    pub business: String,
    pub direction: Direction,
    pub bytes: i64,
    pub count: i64,
    pub recorded_at: String,
}

impl ActiveModelBehavior for ActiveModel {}