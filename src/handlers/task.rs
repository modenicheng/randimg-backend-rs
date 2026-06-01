use axum::{
    Json, Router,
    extract::Path,
    extract::Query,
    extract::State,
    routing::{delete, get},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::auth::middleware::AuthUser;
use crate::db::entities::apalis_job;
use crate::db::query;
use crate::error::AppError;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks", get(list_tasks))
        // NOTE: `/tasks/pending` must be registered before `/tasks/{task_id}` —
        // Axum prioritizes static segments over dynamic params.
        .route("/tasks/pending", delete(delete_pending_tasks))
        .route("/tasks/{task_id}", get(get_task).delete(delete_task))
}

#[derive(Deserialize)]
pub struct ListTasksQuery {
    pub task_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

// ---------------------------------------------------------------------------
// Status mapping (Apalis text ↔ API lowercase)
// ---------------------------------------------------------------------------

/// Map Apalis status string to API status string.
fn map_status(status: &str) -> &'static str {
    match status {
        apalis_job::STATUS_PENDING => "pending",
        apalis_job::STATUS_RUNNING => "running",
        apalis_job::STATUS_DONE => "completed",
        apalis_job::STATUS_FAILED => "failed",
        apalis_job::STATUS_KILLED => "killed",
        _ => "unknown",
    }
}

/// Map API status filter to Apalis status string.
fn unmap_status(status: &str) -> &'static str {
    match status {
        "pending" => apalis_job::STATUS_PENDING,
        "running" => apalis_job::STATUS_RUNNING,
        "completed" => apalis_job::STATUS_DONE,
        "failed" => apalis_job::STATUS_FAILED,
        "killed" => apalis_job::STATUS_KILLED,
        _ => apalis_job::STATUS_PENDING,
    }
}

// ---------------------------------------------------------------------------
// Timestamp formatting (feature-gated for SQLite i64 vs Postgres DateTime)
// ---------------------------------------------------------------------------

/// UTC+8 timezone offset (Asia/Shanghai).
const UTC8: chrono::FixedOffset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();

#[cfg(feature = "sqlite")]
fn fmt_ts(ts: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.with_timezone(&UTC8).format("%Y-%m-%d %H:%M:%S").to_string())
}

#[cfg(feature = "postgres")]
fn fmt_ts(ts: chrono::DateTime<chrono::Utc>) -> String {
    ts.with_timezone(&UTC8).format("%Y-%m-%d %H:%M:%S").to_string()
}

// ---------------------------------------------------------------------------
// row_to_json helper
// ---------------------------------------------------------------------------

fn row_to_json(t: &apalis_job::Model) -> serde_json::Value {
    #[cfg(feature = "sqlite")]
    let run_at = fmt_ts(t.run_at);
    #[cfg(feature = "postgres")]
    let run_at = Some(fmt_ts(t.run_at));

    #[cfg(feature = "sqlite")]
    let done_at = t.done_at.and_then(fmt_ts);
    #[cfg(feature = "postgres")]
    let done_at = t.done_at.map(fmt_ts);

    #[cfg(feature = "sqlite")]
    let last_result = t.last_result.clone();
    #[cfg(feature = "postgres")]
    let last_result = t.last_result.as_ref().map(|v| v.to_string());

    // Deserialize the job BLOB to extract the payload.
    // Apalis JsonCodec<Vec<u8>> serializes the job struct directly (no wrapper).
    let payload = serde_json::from_slice::<serde_json::Value>(&t.job).ok();

    serde_json::json!({
        "id": t.id,
        "job_type": t.job_type,
        "status": map_status(&t.status),
        "priority": t.priority,
        "attempts": t.attempts,
        "max_attempts": t.max_attempts,
        "run_at": run_at,
        "done_at": done_at,
        "last_result": last_result,
        "payload": payload,
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /tasks — List background tasks with optional filters
pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<ListTasksQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let limit = q.limit.unwrap_or(50).min(200);
    let offset = q.offset.unwrap_or(0);

    let db = &state.db;
    let mapped_status = q.status.as_deref().map(unmap_status);

    let (rows, total) = tokio::try_join!(
        query::apalis_job::list(db, q.task_type.as_deref(), mapped_status, limit, offset),
        query::apalis_job::count(db, q.task_type.as_deref(), mapped_status),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let result: Vec<serde_json::Value> = rows.iter().map(row_to_json).collect();

    Ok(Json(serde_json::json!({
        "tasks": result,
        "total": total,
    })))
}

/// GET /tasks/:id — Get a single task by ID
pub async fn get_task(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row = query::apalis_job::find_by_id(&state.db, &task_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    match row {
        Some(t) => Ok(Json(row_to_json(&t))),
        None => Err(AppError::NotFound(format!("Task {} not found", task_id))),
    }
}

/// DELETE /tasks/:id — Delete (cancel) a task by ID
pub async fn delete_task(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = query::apalis_job::delete_by_id(&state.db, &task_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if !deleted {
        return Err(AppError::NotFound(format!("Task {} not found", task_id)));
    }

    Ok(Json(serde_json::json!({ "message": "Task deleted" })))
}

#[derive(Deserialize)]
pub struct DeletePendingQuery {
    pub task_type: Option<String>,
}

/// DELETE /tasks/pending — Delete all pending tasks, optionally filtered by type
pub async fn delete_pending_tasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<DeletePendingQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = query::apalis_job::delete_pending(&state.db, q.task_type.as_deref())
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "message": "Pending tasks deleted",
        "deleted": deleted,
    })))
}
