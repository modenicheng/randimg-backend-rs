use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::middleware::{AuthUser, OptionalAuthUser};
use crate::db::query::image;
use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct ImageQuery {
    pub format: Option<String>,
    pub local: Option<bool>,
}

#[derive(Deserialize)]
pub struct RandomQuery {
    pub format: Option<String>,
    pub local: Option<bool>,
    pub ratio_floor: Option<f32>,
    pub ratio_ceil: Option<f32>,
    pub tags: Option<String>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub offset: Option<u64>,
    pub limit: Option<u64>,
    pub desc: Option<String>,
    pub ratio_floor: Option<f32>,
    pub ratio_ceil: Option<f32>,
    pub author: Option<String>,
    pub accessable: Option<String>,
    pub tags: Option<String>,
}

/// Serve a local image file from disk (with path traversal protection)
async fn serve_local_image(state: &AppState, image_path: &str) -> Result<Response, AppError> {
    let file_path = format!("{}/{}", state.config.image_dir, image_path);

    // Path traversal guard: canonicalize and verify prefix
    let base = std::fs::canonicalize(&state.config.image_dir)
        .map_err(|_| AppError::Internal("Invalid image directory".into()))?;
    let full = std::fs::canonicalize(&file_path)
        .map_err(|_| AppError::NotFound("Image file not found".into()))?;
    if !full.starts_with(&base) {
        return Err(AppError::BadRequest("Invalid image path".into()));
    }

    let bytes = tokio::fs::read(&full)
        .await
        .map_err(|_| AppError::NotFound("Image file not found".into()))?;
    Ok(axum::response::Response::builder()
        .header("Content-Type", "image/jpeg")
        .body(axum::body::Body::from(bytes))
        .unwrap())
}

/// Format image response based on format and local query params
async fn format_image_response(
    img: &serde_json::Value,
    state: &AppState,
    format: &str,
    local: bool,
) -> Result<Response, AppError> {
    if local {
        let path = img["image_path"].as_str().unwrap();
        return serve_local_image(state, path).await;
    }

    if format == "image" {
        let src = img["src"].as_str().unwrap();
        Ok(Redirect::temporary(src).into_response())
    } else {
        Ok(Json(img).into_response())
    }
}

/// GET /  Random image
pub async fn random_image(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RandomQuery>,
) -> Result<Response, AppError> {
    let ratio_floor = query.ratio_floor.unwrap_or(0.0);
    let ratio_ceil = query.ratio_ceil.unwrap_or(10.0);

    let img = image::random_image(
        &state.db,
        ratio_floor,
        ratio_ceil,
        query.tags.as_deref(),
        &state.config,
    )
    .await
    .map_err(AppError::from)?;

    let img = img.ok_or(AppError::NotFound("No image found".into()))?;

    let format = query.format.as_deref().unwrap_or("json");
    let local = query.local.unwrap_or(false);

    format_image_response(&img, &state, format, local).await
}

/// GET /image/{image_id}  Get image by ID
#[axum::debug_handler]
pub async fn get_image(
    State(state): State<Arc<AppState>>,
    Path(image_id): Path<i32>,
    Query(query): Query<ImageQuery>,
    auth: OptionalAuthUser,
) -> Result<Response, AppError> {
    let is_admin = auth.username.is_some();

    let img = image::find_by_id(&state.db, image_id, is_admin, &state.config)
        .await
        .map_err(AppError::from)?;

    let img = img.ok_or(AppError::NotFound("image not found".into()))?;

    let format = query.format.as_deref().unwrap_or("json");
    let local = query.local.unwrap_or(false);

    format_image_response(&img, &state, format, local).await
}

/// GET /list  Paginated image list
#[axum::debug_handler]
pub async fn list_images(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
    auth: OptionalAuthUser,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let is_admin = auth.username.is_some();

    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(30).min(300);

    let desc = query
        .desc
        .as_deref()
        .map(|d| d.to_lowercase() == "true")
        .unwrap_or(true);

    let accessable = if is_admin {
        match query.accessable.as_deref() {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        }
    } else {
        Some(true)
    };

    let result = image::list_images(
        &state.db,
        offset,
        limit,
        desc,
        query.ratio_floor.unwrap_or(0.0),
        query.ratio_ceil.unwrap_or(10.0),
        query.author.as_deref(),
        accessable,
        query.tags.as_deref(),
        is_admin,
        &state.config,
    )
    .await
    .map_err(AppError::from)?;

    Ok(Json(result))
}

#[derive(Deserialize, serde::Serialize)]
pub struct UpdateImageRequest {
    pub id: Option<i32>,
    pub title: Option<String>,
    pub accessable: Option<serde_json::Value>,
    pub processed: Option<bool>,
    pub processing: Option<bool>,
    pub downloaded: Option<bool>,
    pub uploaded: Option<bool>,
    pub colors: Option<serde_json::Value>,
}

/// PATCH /image/{image_id}
pub async fn patch_image(
    State(state): State<Arc<AppState>>,
    Path(image_id): Path<i32>,
    _auth: AuthUser,
    Json(body): Json<UpdateImageRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let data = serde_json::to_value(&body).unwrap_or_default();
    let updated = image::update_fields(&state.db, image_id, data)
        .await
        .map_err(AppError::from)?;

    let updated = updated.ok_or(AppError::NotFound("image not found".into()))?;

    Ok(Json(serde_json::json!({
        "id": updated.id,
        "title": updated.title,
        "accessable": updated.accessable,
        "processed": updated.processed,
        "processing": updated.processing,
    })))
}

/// DELETE /image/{image_id}
pub async fn delete_image(
    State(state): State<Arc<AppState>>,
    Path(image_id): Path<i32>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use crate::db::entities::image::Entity as ImageEntity;
    use crate::db::entities::image_tag_association::{self, Entity as AssocEntity};

    // Delete tag associations first
    AssocEntity::delete_many()
        .filter(image_tag_association::Column::ImageId.eq(image_id))
        .exec(&state.db)
        .await
        .map_err(AppError::from)?;

    let result = ImageEntity::delete_by_id(image_id)
        .exec(&state.db)
        .await
        .map_err(AppError::from)?;

    if result.rows_affected == 0 {
        return Err(AppError::NotFound("image not found".into()));
    }

    Ok(Json(serde_json::json!({ "status": "ok" })))
}
