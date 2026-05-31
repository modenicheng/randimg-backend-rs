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
    pub width_floor: Option<i32>,
    pub width_ceil: Option<i32>,
    pub height_floor: Option<i32>,
    pub height_ceil: Option<i32>,
    pub author: Option<String>,
    pub tags: Option<String>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub offset: Option<u64>,
    pub limit: Option<u64>,
    pub desc: Option<String>,
    pub sort_by: Option<String>,
    pub ratio_floor: Option<f32>,
    pub ratio_ceil: Option<f32>,
    pub width_floor: Option<i32>,
    pub width_ceil: Option<i32>,
    pub height_floor: Option<i32>,
    pub height_ceil: Option<i32>,
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
    let width_floor = query.width_floor.unwrap_or(0);
    let width_ceil = query.width_ceil.unwrap_or(i32::MAX);
    let height_floor = query.height_floor.unwrap_or(0);
    let height_ceil = query.height_ceil.unwrap_or(i32::MAX);

    let img = image::random_image(
        &state.db,
        ratio_floor,
        ratio_ceil,
        width_floor,
        width_ceil,
        height_floor,
        height_ceil,
        query.author.as_deref(),
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
        query.sort_by.as_deref().unwrap_or("id"),
        query.ratio_floor.unwrap_or(0.0),
        query.ratio_ceil.unwrap_or(10.0),
        query.width_floor.unwrap_or(0),
        query.width_ceil.unwrap_or(i32::MAX),
        query.height_floor.unwrap_or(0),
        query.height_ceil.unwrap_or(i32::MAX),
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
    pub is_public: Option<bool>,
    pub avatar_available: Option<bool>,
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
        "is_public": updated.is_public,
        "avatar_available": updated.avatar_available,
    })))
}

/// DELETE /image/{image_id}

#[derive(Deserialize)]
pub struct ColorSearchQuery {
    pub r: Option<u8>,
    pub g: Option<u8>,
    pub b: Option<u8>,
    pub l: Option<f64>,
    pub a: Option<f64>,
    pub b_lab: Option<f64>,
    pub mode: Option<String>,
    pub max_dist: Option<f64>,
    pub limit: Option<u64>,
}

/// GET /color/search  Search images by color similarity in LAB space
///
/// Accepts either RGB (r,g,b query params) or LAB (l,a,b_lab query params).
/// mode: "primary" (default) or "palette"
/// max_dist: optional squared distance cutoff
/// limit: max results (default 20, max 100)
pub async fn color_search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ColorSearchQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    // Convert RGB to LAB if provided, otherwise use LAB directly
    let lab = if let (Some(r), Some(g), Some(b)) = (query.r, query.g, query.b) {
        crate::color::rgb_to_lab(r, g, b)
    } else if let (Some(l), Some(a), Some(b_lab)) = (query.l, query.a, query.b_lab) {
        [l, a, b_lab]
    } else {
        return Err(AppError::BadRequest(
            "Provide either r,g,b or l,a,b_lab query parameters".into(),
        ));
    };

    let mode = query.mode.as_deref().unwrap_or("primary");
    if mode != "primary" && mode != "palette" {
        return Err(AppError::BadRequest(
            "mode must be 'primary' or 'palette'".into(),
        ));
    }

    let limit = query.limit.unwrap_or(20).min(100);

    let results = image::color_search(
        &state.db,
        lab,
        mode,
        query.max_dist,
        limit,
        &state.config,
    )
    .await
    .map_err(AppError::from)?;

    Ok(Json(results))
}
pub async fn delete_image(
    State(state): State<Arc<AppState>>,
    Path(image_id): Path<i32>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use crate::db::entities::image::Entity as ImageEntity;
    use crate::db::entities::image_tag_association::{self, Entity as TagAssocEntity};
    use crate::db::entities::image_color_palette::{self, Entity as PaletteEntity};

    // Delete tag associations
    TagAssocEntity::delete_many()
        .filter(image_tag_association::Column::ImageId.eq(image_id))
        .exec(&state.db)
        .await
        .map_err(AppError::from)?;

    // Delete color palette entries
    PaletteEntity::delete_many()
        .filter(image_color_palette::Column::ImageId.eq(image_id))
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
