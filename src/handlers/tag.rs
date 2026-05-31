use axum::{extract::State, Json};
use std::sync::Arc;

use crate::db::query;
use crate::error::AppError;
use crate::AppState;

pub async fn get_tags(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let tags = query::tag::find_all(&state.db)
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
