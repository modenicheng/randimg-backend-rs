use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::WorkerState;
use crate::auth::middleware::{AuthUser, OptionalAuthUser};
use crate::db::query::image;
use crate::db::query::image::ColorFilterParams;
use crate::error::AppError;

pub fn routes() -> Router<Arc<WorkerState>> {
    Router::new()
        .route("/", get(random_image))
        .route(
            "/image/{image_id}",
            get(get_image).patch(patch_image).delete(delete_image),
        )
        .route("/list", get(list_images))
        .route("/color/search", get(color_search))
}

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
    // Color filter
    pub rgb: Option<String>,
    pub lab: Option<String>,
    pub mode: Option<String>,
    pub max_dist: Option<f64>,
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
    pub accessible: Option<String>,
    pub tags: Option<String>,
    // Color filter
    pub rgb: Option<String>,
    pub lab: Option<String>,
    pub mode: Option<String>,
    pub max_dist: Option<f64>,
}

/// Serve a local image file from disk (with path traversal protection)
async fn serve_local_image(state: &WorkerState, image_path: &str) -> Result<Response, AppError> {
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
    let mime = mime_guess::from_path(&full).first_or_octet_stream();
    Ok(axum::response::Response::builder()
        .header("Content-Type", mime.as_ref())
        .body(axum::body::Body::from(bytes))
        .unwrap())
}

/// Format image response based on format and local query params
async fn format_image_response(
    img: &serde_json::Value,
    state: &WorkerState,
    format: &str,
    local: bool,
) -> Result<Response, AppError> {
    if local {
        let path = img["image_path"]
            .as_str()
            .ok_or_else(|| AppError::Internal("Missing image_path".into()))?;
        return serve_local_image(state, path).await;
    }

    if format == "image" {
        let src = img["src"]
            .as_str()
            .ok_or_else(|| AppError::Internal("Missing src".into()))?;
        Ok(Redirect::temporary(src).into_response())
    } else {
        Ok(Json(img).into_response())
    }
}

/// Parse color filter params from query, converting RGB to LAB if needed.
/// Accepts comma-separated strings: `rgb=r,g,b` or `lab=l,a,b`.
/// Returns `ColorFilterParams` or `None` if no valid color input.
fn parse_color_params(
    rgb: Option<String>,
    lab: Option<String>,
    mode: Option<String>,
    max_dist: Option<f64>,
) -> Option<ColorFilterParams> {
    use crate::db::query::image::DEFAULT_MAX_DIST;

    let lab_color = if let Some(ref rgb_str) = rgb {
        let parts: Vec<&str> = rgb_str.split(',').collect();
        if parts.len() != 3 {
            return None;
        }
        let r: u8 = parts[0].trim().parse().ok()?;
        let g: u8 = parts[1].trim().parse().ok()?;
        let b: u8 = parts[2].trim().parse().ok()?;
        let lab = crate::color::rgb_to_lab(r, g, b);
        [lab[0] as f64, lab[1] as f64, lab[2] as f64]
    } else if let Some(ref lab_str) = lab {
        let parts: Vec<&str> = lab_str.split(',').collect();
        if parts.len() != 3 {
            return None;
        }
        let l: f64 = parts[0].trim().parse().ok()?;
        let a: f64 = parts[1].trim().parse().ok()?;
        let b_lab: f64 = parts[2].trim().parse().ok()?;
        [l, a, b_lab]
    } else {
        return None;
    };

    let mode = mode.unwrap_or_else(|| "primary".to_string());
    if mode != "primary" && mode != "palette" {
        return None;
    }

    let max_dist = max_dist.unwrap_or(DEFAULT_MAX_DIST);

    Some(ColorFilterParams {
        lab: lab_color,
        mode,
        max_dist,
    })
}

/// GET /  Random image
pub async fn random_image(
    State(state): State<Arc<WorkerState>>,
    Query(query): Query<RandomQuery>,
) -> Result<Response, AppError> {
    let ratio_floor = query.ratio_floor.unwrap_or(0.0).max(0.0);
    let ratio_ceil = query.ratio_ceil.unwrap_or(10.0).max(ratio_floor);
    let width_floor = query.width_floor.unwrap_or(0);
    let width_ceil = query.width_ceil.unwrap_or(i32::MAX);
    let height_floor = query.height_floor.unwrap_or(0);
    let height_ceil = query.height_ceil.unwrap_or(i32::MAX);

    let color_params = parse_color_params(query.rgb, query.lab, query.mode, query.max_dist);

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
        color_params,
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
    State(state): State<Arc<WorkerState>>,
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
    State(state): State<Arc<WorkerState>>,
    Query(query): Query<ListQuery>,
    auth: OptionalAuthUser,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let is_admin = auth.username.is_some();

    let offset = query.offset.unwrap_or(0).min(100_000);
    let limit = query.limit.unwrap_or(30).min(300);

    let desc = query
        .desc
        .as_deref()
        .map(|d| d.to_lowercase() == "true")
        .unwrap_or(true);

    let accessible = if is_admin {
        match query.accessible.as_deref() {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        }
    } else {
        Some(true)
    };

    let sort_by = query.sort_by.as_deref().unwrap_or("id");
    let allowed_sorts = [
        "id",
        "width",
        "height",
        "aspect_ratio",
        "source_created_at",
        "created_at",
        "popularity",
        "distance",
    ];
    if !allowed_sorts.contains(&sort_by) {
        return Err(AppError::BadRequest(format!(
            "Invalid sort_by '{}'. Allowed: {}",
            sort_by,
            allowed_sorts.join(", ")
        )));
    }

    let color_params = parse_color_params(query.rgb, query.lab, query.mode, query.max_dist);

    let result = image::list_images(
        &state.db,
        offset,
        limit,
        desc,
        sort_by,
        query.ratio_floor.unwrap_or(0.0),
        query.ratio_ceil.unwrap_or(10.0),
        query.width_floor.unwrap_or(0),
        query.width_ceil.unwrap_or(i32::MAX),
        query.height_floor.unwrap_or(0),
        query.height_ceil.unwrap_or(i32::MAX),
        query.author.as_deref(),
        accessible,
        query.tags.as_deref(),
        color_params,
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
    pub accessible: Option<serde_json::Value>,
    pub is_public: Option<bool>,
    pub avatar_available: Option<bool>,
    pub colors: Option<serde_json::Value>,
}

/// PATCH /image/{image_id}
pub async fn patch_image(
    State(state): State<Arc<WorkerState>>,
    Path(image_id): Path<i32>,
    _auth: AuthUser,
    Json(body): Json<UpdateImageRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Validate accessible: must be boolean or null
    if let Some(ref val) = body.accessible {
        if !val.is_boolean() && !val.is_null() {
            return Err(AppError::BadRequest(
                "accessible must be a boolean or null".into(),
            ));
        }
    }

    // Validate colors: must be an object or array
    if let Some(ref val) = body.colors {
        if !val.is_object() && !val.is_array() {
            return Err(AppError::BadRequest(
                "colors must be a JSON object or array".into(),
            ));
        }
    }

    // Validate title: must not be empty string and not exceed 1000 chars
    if let Some(ref title) = body.title {
        if title.is_empty() {
            return Err(AppError::BadRequest(
                "title must not be an empty string".into(),
            ));
        }
        if title.len() > 1000 {
            return Err(AppError::BadRequest(
                "title must not exceed 1000 characters".into(),
            ));
        }
    }

    let data = serde_json::to_value(&body).unwrap_or_default();
    let updated = image::update_fields(&state.db, image_id, data)
        .await
        .map_err(AppError::from)?;

    let updated = updated.ok_or(AppError::NotFound("image not found".into()))?;

    Ok(Json(serde_json::json!({
        "id": updated.id,
        "title": updated.title,
        "accessible": updated.accessible,
        "is_public": updated.is_public,
        "avatar_available": updated.avatar_available,
    })))
}

/// DELETE /image/{image_id}

#[derive(Deserialize)]
pub struct ColorSearchQuery {
    pub rgb: Option<String>,
    pub lab: Option<String>,
    pub mode: Option<String>,
    pub max_dist: Option<f64>,
    pub limit: Option<u64>,
}

/// GET /color/search  Search images by color similarity in LAB space
///
/// Accepts either RGB (?rgb=255,0,0) or LAB (?lab=50,0,0) comma-separated query params.
/// mode: "primary" (default) or "palette"
/// max_dist: optional squared distance cutoff
/// limit: max results (default 20, max 100)
pub async fn color_search(
    State(state): State<Arc<WorkerState>>,
    Query(query): Query<ColorSearchQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let lab_color = if let Some(ref rgb_str) = query.rgb {
        let parts: Vec<&str> = rgb_str.split(',').collect();
        if parts.len() != 3 {
            return Err(AppError::BadRequest(
                "Invalid rgb format. Use rgb=r,g,b with three comma-separated values".into(),
            ));
        }
        let r: u8 = parts[0]
            .trim()
            .parse()
            .map_err(|_| AppError::BadRequest("Invalid red value in rgb parameter".into()))?;
        let g: u8 = parts[1]
            .trim()
            .parse()
            .map_err(|_| AppError::BadRequest("Invalid green value in rgb parameter".into()))?;
        let b: u8 = parts[2]
            .trim()
            .parse()
            .map_err(|_| AppError::BadRequest("Invalid blue value in rgb parameter".into()))?;
        let lab = crate::color::rgb_to_lab(r, g, b);
        [lab[0] as f64, lab[1] as f64, lab[2] as f64]
    } else if let Some(ref lab_str) = query.lab {
        let parts: Vec<&str> = lab_str.split(',').collect();
        if parts.len() != 3 {
            return Err(AppError::BadRequest(
                "Invalid lab format. Use lab=l,a,b with three comma-separated values".into(),
            ));
        }
        let l: f64 = parts[0]
            .trim()
            .parse()
            .map_err(|_| AppError::BadRequest("Invalid L value in lab parameter".into()))?;
        let a: f64 = parts[1]
            .trim()
            .parse()
            .map_err(|_| AppError::BadRequest("Invalid A value in lab parameter".into()))?;
        let b_lab: f64 = parts[2]
            .trim()
            .parse()
            .map_err(|_| AppError::BadRequest("Invalid B value in lab parameter".into()))?;
        [l, a, b_lab]
    } else {
        return Err(AppError::BadRequest(
            "Provide either rgb=r,g,b or lab=l,a,b query parameters".into(),
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
        lab_color,
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
    State(state): State<Arc<WorkerState>>,
    Path(image_id): Path<i32>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    use crate::db::entities::image::Entity as ImageEntity;
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};

    let img = ImageEntity::find_by_id(image_id)
        .one(&state.db)
        .await
        .map_err(AppError::from)?;

    let Some(img) = img else {
        return Err(AppError::NotFound("image not found".into()));
    };

    // Best-effort remove physical file from disk
    let file_path = format!("{}/{}", state.config.image_dir, img.image_path);

    // Path traversal guard: canonicalize and verify prefix
    let base = std::fs::canonicalize(&state.config.image_dir)
        .map_err(|_| AppError::Internal("Invalid image directory".into()))?;
    let full = std::fs::canonicalize(&file_path)
        .map_err(|_| AppError::NotFound("Image file not found".into()))?;
    if !full.starts_with(&base) {
        return Err(AppError::BadRequest("Invalid image path".into()));
    }

    match tokio::fs::remove_file(&full).await {
        Ok(_) => {
            tracing::info!("Deleted image file: {}", file_path);
        }
        Err(e) => {
            tracing::warn!("Failed to delete image file {}: {}", file_path, e);
        }
    }

    // Soft delete: set deleted_at and mark as not public
    let mut active = img.into_active_model();
    active.deleted_at = Set(Some(Utc::now().fixed_offset()));
    active.is_public = Set(false);
    active.update(&state.db).await.map_err(AppError::from)?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}
