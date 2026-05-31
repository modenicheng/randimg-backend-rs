use sea_orm::*;
use chrono::NaiveDateTime;
use crate::db::entities::crawler::{self, Entity as Crawler};

pub async fn create(
    db: &DatabaseConnection,
    task_name: &str,
    crawl_type: i32,
    target_user_id: Option<&str>,
    target_start_date: Option<NaiveDateTime>,
    target_end_date: Option<NaiveDateTime>,
    target_search_prompt: Option<&str>,
) -> Result<crawler::Model, DbErr> {
    let model = crawler::ActiveModel {
        task_name: Set(task_name.to_string()),
        crawl_type: Set(crawl_type),
        status: Set(0),
        target_user_id: Set(target_user_id.map(|s| s.to_string())),
        target_start_date: Set(target_start_date),
        target_end_date: Set(target_end_date),
        target_search_prompt: Set(target_search_prompt.map(|s| s.to_string())),
        ..Default::default()
    };
    model.insert(db).await
}

pub async fn find_all(db: &DatabaseConnection) -> Result<Vec<crawler::Model>, DbErr> {
    Crawler::find().all(db).await
}
