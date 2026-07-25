use sea_orm::entity::prelude::*;
use crate::types::{ObjectKey, ThumbnailFormat};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "thumbnails")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub file_key: ObjectKey,
    pub data: Vec<u8>,
    pub format: ThumbnailFormat,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub cached_at: String,
}

impl ActiveModelBehavior for ActiveModel {}