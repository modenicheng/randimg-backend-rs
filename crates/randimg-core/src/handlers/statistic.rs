use axum::{Json, Router, extract::State, routing::get};
use sea_orm::{EntityTrait, PaginatorTrait};
use std::sync::Arc;

use crate::WorkerState;
use crate::db::query;
use crate::error::AppError;

pub fn routes() -> Router<Arc<WorkerState>> {
    Router::new().route("/statistic", get(get_statistic))
}

pub async fn get_statistic(
    State(state): State<Arc<WorkerState>>,
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
