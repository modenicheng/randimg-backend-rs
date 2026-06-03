use axum::{Json, Router, routing::get};
use std::sync::Arc;

use crate::WorkerState;

pub fn routes() -> Router<Arc<WorkerState>> {
    Router::new().route("/health", get(health_check))
}

pub async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
