use axum::{
    Json, Router,
    extract::Path,
    extract::Query,
    extract::State,
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::auth::middleware::AuthUser;
use crate::db::entities::image::{self, Entity as Image};
use crate::db::query;
use crate::error::AppError;
use crate::task_queue::jobs::*;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/crawler", get(list_crawlers).post(create_crawler))
        .route("/crawler/image", get(get_crawler_image))
        .route("/admin/accessibility-queue", get(get_accessibility_queue))
        .route("/crawler/discover", post(trigger_discover))
        .route("/crawler/{crawler_id}", get(get_crawler))
}

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
    if crawl_type == 0 && (body.target_start_date.is_none() || body.target_end_date.is_none()) {
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

    // Submit crawl task to Apalis job queue
    state
        .job_storage
        .push_crawl(CrawlJob {
            crawler_id: crawler.id,
            crawl_type,
            target_user_id: body.target_user_id,
            target_start_date: body.target_start_date.map(|d| d.to_string()),
            target_end_date: body.target_end_date.map(|d| d.to_string()),
            target_search_prompt: body.target_search_prompt,
        })
        .await
        .map_err(|e| AppError::Internal(e))?;

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
            state
                .job_storage
                .push_color_extract(ColorExtractJob {
                    image_id: img.id,
                    image_path: img.image_path,
                })
                .await
                .map_err(|e| AppError::Internal(e))?;
        }

        return Ok(Json(serde_json::json!({
            "status": "ok",
            "count": count,
            "message": "Color extraction jobs submitted to Apalis queue",
        })));
    }

    // With Apalis, tasks are processed automatically by workers.
    // No manual task claiming needed.
    Err(AppError::BadRequest(
        "Use ?init=true to submit color extraction jobs. Processing is automatic via Apalis workers.".into(),
    ))
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
    state
        .job_storage
        .push_discover(DiscoverJob {
            hop: 0,
            max_hops: body.max_hops,
            seed_limit: body.seed_limit,
            seed_method: body.seed_method,
        })
        .await
        .map_err(|e| AppError::Internal(e))?;

    Ok(Json(serde_json::json!({
        "status": "ok",
        "message": "Discover task submitted",
    })))
}

/// GET /admin/accessibility-queue  Get images for accessibility check
pub async fn get_accessibility_queue(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(query): Query<CrawlerImageQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if query.init.unwrap_or(false) {
        use sea_orm::*;
        let images = Image::find()
            .filter(image::Column::IsPublic.eq(true))
            .filter(image::Column::Accessible.is_null())
            .all(&state.db)
            .await
            .map_err(AppError::from)?;

        let count = images.len();
        for img in images {
            state
                .job_storage
                .push_accessibility_check(AccessibilityCheckJob {
                    image_id: img.id,
                    image_path: img.image_path,
                })
                .await
                .map_err(|e| AppError::Internal(e))?;
        }

        return Ok(Json(serde_json::json!({
            "status": "ok",
            "count": count,
            "message": "Accessibility check jobs submitted to Apalis queue",
        })));
    }

    // With Apalis, tasks are processed automatically by workers.
    Err(AppError::BadRequest(
        "Use ?init=true to submit accessibility check jobs. Processing is automatic via Apalis workers.".into(),
    ))
}

/// GET /crawler/{crawler_id}  Get single crawler detail
pub async fn get_crawler(
    State(state): State<Arc<AppState>>,
    Path(crawler_id): Path<i32>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let crawler = query::crawler::find_by_id(&state.db, crawler_id)
        .await
        .map_err(AppError::from)?;

    let c = crawler.ok_or(AppError::NotFound("Crawler not found".into()))?;

    Ok(Json(serde_json::json!({
        "id": c.id,
        "task_name": c.task_name,
        "crawl_type": c.crawl_type,
        "status": c.status,
        "start_time": c.start_time,
        "end_time": c.end_time,
        "total_pages": c.total_pages,
        "processed_pages": c.processed_pages,
        "target_user_id": c.target_user_id,
        "target_start_date": c.target_start_date,
        "target_end_date": c.target_end_date,
        "target_search_prompt": c.target_search_prompt,
    })))
}
