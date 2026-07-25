use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "classification_rules")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub extension: String,
    pub file_type: String,
    pub priority: i32,
    pub description: Option<String>,
}

impl ActiveModelBehavior for ActiveModel {}