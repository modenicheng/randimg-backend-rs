use std::sync::Arc;

use axum::extract::{Path, State};

use crate::AppState;

pub async fn random_image(State(_state): State<Arc<AppState>>) -> &'static str {
    todo!("random_image handler")
}

pub async fn get_image(
    State(_state): State<Arc<AppState>>,
    Path(_image_id): Path<String>,
) -> &'static str {
    todo!("get_image handler")
}

pub async fn list_images(State(_state): State<Arc<AppState>>) -> &'static str {
    todo!("list_images handler")
}
