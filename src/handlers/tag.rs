use axum::{extract::Path, extract::Query, extract::State, Json};
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::db::query;
use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct TagQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

pub async fn get_tags(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TagQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let tags = query::tag::find_all(&state.db, q.limit, q.offset)
        .await
        .map_err(AppError::from)?;

    let result: Vec<serde_json::Value> = tags
        .into_iter()
        .map(|t| {
            let search_string = format!(
                "{}|{}",
                t.name,
                t.translated_name.as_deref().unwrap_or("")
            );
            serde_json::json!({
                "id": t.id,
                "name": t.name,
                "translated_name": t.translated_name,
                "search_string": search_string,
            })
        })
        .collect();

    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct UpdateTagRequest {
    pub translated_name: Option<String>,
}

pub async fn update_tag(
    State(state): State<Arc<AppState>>,
    Path(tag_id): Path<i32>,
    _auth: AuthUser,
    Json(body): Json<UpdateTagRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tag = query::tag::update_tag(&state.db, tag_id, body.translated_name.as_deref())
        .await
        .map_err(AppError::from)?;

    let tag = tag.ok_or(AppError::NotFound("Tag not found".into()))?;

    Ok(Json(serde_json::json!({
        "id": tag.id,
        "name": tag.name,
        "translated_name": tag.translated_name,
    })))
}

pub async fn delete_tag(
    State(state): State<Arc<AppState>>,
    Path(tag_id): Path<i32>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use crate::db::entities::image_tag_association::{self, Entity as AssocEntity};

    // Remove associations first
    AssocEntity::delete_many()
        .filter(image_tag_association::Column::TagId.eq(tag_id))
        .exec(&state.db)
        .await
        .map_err(AppError::from)?;

    let result = crate::db::entities::tag::Entity::delete_by_id(tag_id)
        .exec(&state.db)
        .await
        .map_err(AppError::from)?;

    if result.rows_affected == 0 {
        return Err(AppError::NotFound("Tag not found".into()));
    }

    Ok(Json(serde_json::json!({ "status": "ok" })))
}
