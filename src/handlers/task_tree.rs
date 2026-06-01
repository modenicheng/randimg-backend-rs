use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use std::sync::Arc;

use crate::AppState;
use crate::auth::middleware::AuthUser;
use crate::db::query;
use crate::error::AppError;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks/{task_id}/tree", get(get_task_tree))
        .route("/tasks/{task_id}/children", get(get_task_children))
        .route("/tasks/{task_id}/parent", get(get_task_parent))
}

/// GET /tasks/{task_id}/tree
///
/// Returns the full task hierarchy tree rooted at the given task ID.
/// Returns an empty array if the task has no children.
pub async fn get_task_tree(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tree = query::task_dependency::get_task_tree(&state.db, &task_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "root_job_id": task_id,
        "children": tree,
    })))
}

/// GET /tasks/{task_id}/children
///
/// Returns the direct children of a task.
pub async fn get_task_children(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let children = query::task_dependency::get_children(&state.db, &task_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "parent_job_id": task_id,
        "children": children,
    })))
}

/// GET /tasks/{task_id}/parent
///
/// Returns the parent of a task, or null if the task is a root task.
pub async fn get_task_parent(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let parent = query::task_dependency::get_parent(&state.db, &task_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "child_job_id": task_id,
        "parent": parent,
    })))
}
