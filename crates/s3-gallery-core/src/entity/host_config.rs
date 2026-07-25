use sea_orm::entity::prelude::*;
use crate::types::HostId;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "host_config")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,
    pub host_name: String,
    pub host_type: String,
    pub description: String,
    pub created_at: String,
    pub bucket: String,
    pub endpoint: String,
    pub region: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::file::Entity")]
    Files,
}

impl ActiveModelBehavior for ActiveModel {}