use crate::db::entities::crawler::{self, Entity as Crawler};
use chrono::{NaiveDateTime, Utc};
use sea_orm::*;

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

/// Mark a crawler as running (status=1) and record start_time.
pub async fn mark_running(db: &DatabaseConnection, crawler_id: i32) -> Result<(), DbErr> {
    if let Some(c) = Crawler::find_by_id(crawler_id).one(db).await? {
        let mut active: crawler::ActiveModel = c.into();
        active.status = Set(1);
        active.start_time = Set(Some(Utc::now().naive_utc()));
        active.update(db).await?;
    }
    Ok(())
}

/// Mark a crawler as completed (status=2) and record end_time + total_pages.
pub async fn mark_completed(
    db: &DatabaseConnection,
    crawler_id: i32,
    total_pages: i32,
) -> Result<(), DbErr> {
    if let Some(c) = Crawler::find_by_id(crawler_id).one(db).await? {
        let mut active: crawler::ActiveModel = c.into();
        active.status = Set(2);
        active.end_time = Set(Some(Utc::now().naive_utc()));
        active.total_pages = Set(Some(total_pages));
        active.processed_pages = Set(Some(total_pages));
        active.update(db).await?;
    }
    Ok(())
}

/// Mark a crawler as failed (status=99) and record end_time.
pub async fn mark_failed(db: &DatabaseConnection, crawler_id: i32) -> Result<(), DbErr> {
    if let Some(c) = Crawler::find_by_id(crawler_id).one(db).await? {
        let mut active: crawler::ActiveModel = c.into();
        active.status = Set(99);
        active.end_time = Set(Some(Utc::now().naive_utc()));
        active.update(db).await?;
    }
    Ok(())
}

/// Find a single crawler by ID.
pub async fn find_by_id(
    db: &DatabaseConnection,
    crawler_id: i32,
) -> Result<Option<crawler::Model>, DbErr> {
    Crawler::find_by_id(crawler_id).one(db).await
}
