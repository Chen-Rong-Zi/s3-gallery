use sea_orm::entity::prelude::*;
use crate::types::{HostId, ObjectKey};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "traffic_file_log")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub id: i64,
    pub host_id: HostId,
    pub file_key: ObjectKey,
    pub business: String,
    pub bytes: i64,
    pub count: i64,
    pub recorded_at: String,
}

impl ActiveModelBehavior for ActiveModel {}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}