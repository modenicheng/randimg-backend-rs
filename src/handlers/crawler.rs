use axum::{extract::State, Json};
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::db::query;
use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct CreateCrawlerRequest {
    pub task_name: Option<String>,
    pub crawl_type: Option<i32>,
    pub target_user_id: Option<String>,
    pub target_start_date: Option<chrono::NaiveDateTime>,
    pub target_end_date: Option<chrono::NaiveDateTime>,
    pub target_search_prompt: Option<String>,
}

/// POST /crawler  Create crawler task
pub async fn create_crawler(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<CreateCrawlerRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let crawl_type = body.crawl_type.unwrap_or(1);

    // Validation
    if crawl_type == 1 && body.target_user_id.is_none() {
        return Err(AppError::BadRequest(
            "target_user_id is required for USER crawler".into(),
        ));
    }
    if crawl_type == 0
        && (body.target_start_date.is_none() || body.target_end_date.is_none())
    {
        return Err(AppError::BadRequest(
            "target_end_date and target_start_date is required for RANKING crawler".into(),
        ));
    }

    let crawler = query::crawler::create(
        &state.db,
        body.task_name.as_deref().unwrap_or(""),
        crawl_type,
        body.target_user_id.as_deref(),
        body.target_start_date,
        body.target_end_date,
        body.target_search_prompt.as_deref(),
    )
    .await
    .map_err(AppError::from)?;

    Ok(Json(serde_json::json!({
        "id": crawler.id,
        "task_name": crawler.task_name,
        "crawl_type": crawler.crawl_type,
        "status": crawler.status,
    })))
}

/// GET /crawler  List crawler tasks
pub async fn list_crawlers(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let crawlers = query::crawler::find_all(&state.db)
        .await
        .map_err(AppError::from)?;

    let result: Vec<serde_json::Value> = crawlers
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "task_name": c.task_name,
                "crawl_type": c.crawl_type,
                "status": c.status,
                "start_time": c.start_time,
                "end_time": c.end_time,
                "total_pages": c.total_pages,
                "processed_pages": c.processed_pages,
            })
        })
        .collect();

    Ok(Json(result))
}
