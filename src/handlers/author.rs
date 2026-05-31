use axum::{extract::Path, extract::Query, extract::State, routing::get, Json, Router};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Deserialize;
use std::sync::Arc;

use crate::db::entities::image::{self, Entity as ImageEntity};
use crate::db::query;
use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/authors", get(list_authors))
        .route("/authors/{author_id}", get(get_author))
}

#[derive(Deserialize)]
pub struct AuthorQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

pub async fn list_authors(
    State(state): State<Arc<AppState>>,
    Query(q): Query<AuthorQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let limit = q.limit.unwrap_or(30).min(300);
    let offset = q.offset.unwrap_or(0);

    let authors = query::author::find_all(&state.db, Some(limit), Some(offset))
        .await
        .map_err(AppError::from)?;

    let result: Vec<serde_json::Value> = authors
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id,
                "name": a.name,
                "platform": a.platform,
                "platform_id": a.platform_id,
                "homepage": a.homepage,
            })
        })
        .collect();

    Ok(Json(result))
}

pub async fn get_author(
    State(state): State<Arc<AppState>>,
    Path(author_id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    let author = query::author::find_by_id(&state.db, author_id)
        .await
        .map_err(AppError::from)?;

    let author = author.ok_or(AppError::NotFound("Author not found".into()))?;

    // Fetch images by this author (paginated, just first 20)
    let images = ImageEntity::find()
        .filter(image::Column::AuthorId.eq(author_id))
        .filter(image::Column::IsPublic.eq(true))
        .filter(image::Column::DeletedAt.is_null())
        .order_by_desc(image::Column::CreatedAt)
        .limit(20)
        .all(&state.db)
        .await
        .map_err(AppError::from)?;

    let image_list: Vec<serde_json::Value> = images
        .into_iter()
        .map(|img| {
            serde_json::json!({
                "id": img.id,
                "title": img.title,
                "src": format!("{}{}", state.config.cdn_base_url, img.image_path),
                "aspect_ratio": img.aspect_ratio,
                "width": img.width,
                "height": img.height,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "id": author.id,
        "name": author.name,
        "platform": author.platform,
        "platform_id": author.platform_id,
        "homepage": author.homepage,
        "images": image_list,
    })))
}
