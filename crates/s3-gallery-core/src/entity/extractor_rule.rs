use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "extractor_rules")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub extension: String,
    #[sea_orm(primary_key)]
    pub extractor_name: String,
    pub priority: i32,
}

impl ActiveModelBehavior for ActiveModel {}