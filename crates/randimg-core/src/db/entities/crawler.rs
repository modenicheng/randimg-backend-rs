use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "crawlers")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub task_name: String,
    pub start_time: Option<NaiveDateTime>,
    pub end_time: Option<NaiveDateTime>,
    pub crawl_type: i32,
    pub status: i32,
    pub total_pages: Option<i32>,
    pub processed_pages: Option<i32>,
    pub target_user_id: Option<String>,
    pub target_start_date: Option<NaiveDateTime>,
    pub target_end_date: Option<NaiveDateTime>,
    pub target_search_prompt: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
