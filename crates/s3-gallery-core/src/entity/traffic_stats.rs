use sea_orm::entity::prelude::*;
use crate::types::{HostId, S3Operation, Direction};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "traffic_stats")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,
    #[sea_orm(primary_key)]
    pub period: String,
    #[sea_orm(primary_key)]
    pub operation: S3Operation,
    #[sea_orm(primary_key)]
    pub business: String,
    #[sea_orm(primary_key)]
    pub direction: Direction,
    pub total_bytes: i64,
    pub total_count: i64,
}

impl ActiveModelBehavior for ActiveModel {}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}