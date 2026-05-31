use axum::{extract::Path, extract::Query, extract::State, Json};
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::db::query;
use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct ListTasksQuery {
    pub task_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u64>,
}

/// GET /tasks — List background tasks with optional filters
pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<ListTasksQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let limit = q.limit.unwrap_or(50).min(200);
    let tasks = query::task::find_filtered(
        &state.db,
        q.task_type.as_deref(),
        q.status.as_deref(),
        limit,
    )
    .await
    .map_err(AppError::from)?;

    let result: Vec<serde_json::Value> = tasks
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "task_type": t.task_type,
                "status": t.status,
                "priority": t.priority,
                "retry_count": t.retry_count,
                "max_retries": t.max_retries,
                "image_id": t.image_id,
                "created_at": t.created_at,
                "started_at": t.started_at,
                "finished_at": t.finished_at,
                "last_error": t.last_error,
            })
        })
        .collect();

    Ok(Json(result))
}

/// GET /tasks/:id — Get a single task by ID
pub async fn get_task(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let task = query::task::find_by_id(&state.db, &task_id)
        .await
        .map_err(AppError::from)?;

    match task {
        Some(t) => Ok(Json(serde_json::json!({
            "id": t.id,
            "task_type": t.task_type,
            "payload": t.payload,
            "status": t.status,
            "priority": t.priority,
            "retry_count": t.retry_count,
            "max_retries": t.max_retries,
            "image_id": t.image_id,
            "image_path": t.image_path,
            "created_at": t.created_at,
            "started_at": t.started_at,
            "finished_at": t.finished_at,
            "last_error": t.last_error,
        }))),
        None => Err(AppError::NotFound(format!("Task {} not found", task_id))),
    }
}

/// POST /tasks/:id/retry — Manually retry a failed task
pub async fn retry_task(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    match query::task::retry_task(&state.db, &task_id)
        .await
        .map_err(AppError::from)?
    {
        Some(t) => Ok(Json(serde_json::json!({
            "id": t.id,
            "status": t.status,
            "message": "Task requeued for retry",
        }))),
        None => Err(AppError::NotFound(format!("Task {} not found", task_id))),
    }
}
