use axum::{extract::State, routing::get, Json, Router};
use sea_orm::{EntityTrait, PaginatorTrait};
use std::sync::Arc;

use crate::db::query;
use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/statistic", get(get_statistic))
}

pub async fn get_statistic(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let illust_count = query::image::count_accessible(&state.db)
        .await
        .map_err(AppError::from)?;

    let tag_count = crate::db::entities::tag::Entity::find()
        .count(&state.db)
        .await
        .map_err(AppError::from)?;

    let author_count = query::author::count(&state.db)
        .await
        .map_err(AppError::from)?;

    Ok(Json(serde_json::json!({
        "illust_count": illust_count,
        "tag_count": tag_count,
        "author_count": author_count,
    })))
}
