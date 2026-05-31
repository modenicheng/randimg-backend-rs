use std::sync::Arc;

use axum::extract::State;

use crate::AppState;

pub async fn get_tags(State(_state): State<Arc<AppState>>) -> &'static str {
    todo!("get_tags handler")
}
