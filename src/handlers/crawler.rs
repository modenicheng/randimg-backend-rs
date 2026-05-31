use axum::{extract::Query, extract::State, Json};
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::db::entities::image::{self, Entity as Image};
use crate::db::query;
use crate::error::AppError;
use crate::task_queue;
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

    // Submit crawl task to background queue
    crate::task_queue::submit_task(
        &state.db,
        "crawl",
        serde_json::json!({
            "crawler_id": crawler.id,
            "crawl_type": crawl_type,
            "target_user_id": body.target_user_id,
            "target_start_date": body.target_start_date.map(|d| d.to_string()),
            "target_end_date": body.target_end_date.map(|d| d.to_string()),
            "target_search_prompt": body.target_search_prompt,
        }),
        1, // Higher priority than download/color tasks
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

#[derive(Deserialize)]
pub struct CrawlerImageQuery {
    pub init: Option<bool>,
}

/// GET /crawler/image  Get images for processing
pub async fn get_crawler_image(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(query): Query<CrawlerImageQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if query.init.unwrap_or(false) {
        let images = query::image::find_unprocessed(&state.db)
            .await
            .map_err(AppError::from)?;

        let count = images.len();
        for img in images {
            task_queue::submit_task(
                &state.db,
                "color_extract",
                serde_json::json!({
                    "image_id": img.id,
                    "image_path": img.image_path,
                }),
                0,
            )
            .await
            .map_err(AppError::from)?;
        }

        return Ok(Json(serde_json::json!({
            "status": "ok",
            "count": count,
        })));
    }

    let task = task_queue::claim_next_task(&state.db, "color_extract")
        .await
        .map_err(AppError::from)?;

    match task {
        Some(t) => {
            // Use image_id from task field or payload
            let image_id = t.image_id
                .or_else(|| t.payload["image_id"].as_i64().map(|v| v as i32));
            let image_path = t.image_path
                .as_deref()
                .or_else(|| t.payload["image_path"].as_str());

            Ok(Json(serde_json::json!({
                "id": image_id,
                "image_path": image_path,
                "task_id": t.id,
            })))
        }
        None => Err(AppError::NotFound(
            "No image found. Please try init first.".into(),
        )),
    }
}

#[derive(Deserialize)]
pub struct ErrorCrawlerImageRequest {
    pub task_id: Option<String>,
    pub id: Option<i64>,
}

/// POST /crawler/image  Error callback
pub async fn error_crawler_image(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<ErrorCrawlerImageRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(ref task_id) = body.task_id {
        task_queue::fail_task(&state.db, task_id, "requeued by worker")
            .await
            .map_err(AppError::from)?;
    }

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[derive(Deserialize)]
pub struct DiscoverRequest {
    pub max_hops: Option<u32>,
    pub seed_limit: Option<u64>,
    pub seed_method: Option<String>,
}

/// POST /crawler/discover  Manually trigger a discover crawl
pub async fn trigger_discover(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<DiscoverRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let payload = serde_json::json!({
        "hop": 0,
        "max_hops": body.max_hops,
        "seed_limit": body.seed_limit,
        "seed_method": body.seed_method,
    });

    task_queue::submit_task(&state.db, "discover", payload, 0)
        .await
        .map_err(AppError::from)?;

    Ok(Json(serde_json::json!({
        "status": "ok",
        "message": "Discover task submitted",
    })))
}

/// GET /adjust-accessible  Get images for accessibility check
pub async fn get_adjust_accessible(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(query): Query<CrawlerImageQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if query.init.unwrap_or(false) {
        use sea_orm::*;
        let images = Image::find()
            .filter(image::Column::IsPublic.eq(true))
            .filter(image::Column::Accessable.is_null())
            .all(&state.db)
            .await
            .map_err(AppError::from)?;

        let count = images.len();
        for img in images {
            task_queue::submit_task(
                &state.db,
                "accessibility_check",
                serde_json::json!({
                    "image_id": img.id,
                    "image_path": img.image_path,
                }),
                0,
            )
            .await
            .map_err(AppError::from)?;
        }

        return Ok(Json(serde_json::json!({
            "status": "ok",
            "count": count,
        })));
    }

    let task = task_queue::claim_next_task(&state.db, "accessibility_check")
        .await
        .map_err(AppError::from)?;

    match task {
        Some(t) => {
            let image_id = t.image_id
                .or_else(|| t.payload["image_id"].as_i64().map(|v| v as i32));
            let image_path = t.image_path
                .as_deref()
                .or_else(|| t.payload["image_path"].as_str());

            Ok(Json(serde_json::json!({
                "id": image_id,
                "image_path": image_path,
                "task_id": t.id,
            })))
        },
        None => Err(AppError::NotFound("Queue empty".into())),
    }
}
