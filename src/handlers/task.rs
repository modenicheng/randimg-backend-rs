use axum::{
    Json, Router,
    extract::Path,
    extract::Query,
    extract::State,
    routing::get,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks", get(list_tasks))
        .route("/tasks/{task_id}", get(get_task))
    // Note: manual retry is no longer needed — Apalis handles retries automatically.
    // Failed tasks can be re-pushed via the relevant API endpoint.
}

#[derive(Deserialize)]
pub struct ListTasksQuery {
    pub task_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

/// Map Apalis status values to human-readable equivalents.
fn map_status(status: &str) -> &str {
    match status {
        "Pending" => "pending",
        "Running" => "running",
        "Done" => "completed",
        "Failed" => "failed",
        "Killed" => "killed",
        other => other,
    }
}

/// Map human-readable status to Apalis status for filtering.
fn unmap_status(status: &str) -> &str {
    match status {
        "pending" => "Pending",
        "running" => "Running",
        "completed" => "Done",
        "failed" => "Failed",
        "killed" => "Killed",
        other => other,
    }
}

/// Convert unix timestamp (seconds) to ISO-like string.
fn ts_to_string(ts: Option<i64>) -> Option<String> {
    ts.map(|t| {
        chrono::DateTime::from_timestamp(t, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| t.to_string())
    })
}

/// GET /tasks — List background tasks with optional filters
pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<ListTasksQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let limit = q.limit.unwrap_or(50).min(200) as i64;
    let offset = q.offset.unwrap_or(0) as i64;

    let pool = &state.apalis_pool;

    // Build query dynamically based on filters
    let rows = match (&q.task_type, &q.status) {
        (Some(tt), Some(st)) => {
            sqlx::query_as::<_, ApalisJobRow>(
                "SELECT id, job_type, status, attempts, max_attempts, run_at, done_at, last_result, priority
                 FROM Jobs WHERE job_type = ?1 AND status = ?2 ORDER BY run_at DESC LIMIT ?3 OFFSET ?4"
            )
            .bind(tt)
            .bind(unmap_status(st))
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (Some(tt), None) => {
            sqlx::query_as::<_, ApalisJobRow>(
                "SELECT id, job_type, status, attempts, max_attempts, run_at, done_at, last_result, priority
                 FROM Jobs WHERE job_type = ?1 ORDER BY run_at DESC LIMIT ?2 OFFSET ?3"
            )
            .bind(tt)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (None, Some(st)) => {
            sqlx::query_as::<_, ApalisJobRow>(
                "SELECT id, job_type, status, attempts, max_attempts, run_at, done_at, last_result, priority
                 FROM Jobs WHERE status = ?1 ORDER BY run_at DESC LIMIT ?2 OFFSET ?3"
            )
            .bind(unmap_status(st))
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (None, None) => {
            sqlx::query_as::<_, ApalisJobRow>(
                "SELECT id, job_type, status, attempts, max_attempts, run_at, done_at, last_result, priority
                 FROM Jobs ORDER BY run_at DESC LIMIT ?1 OFFSET ?2"
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
    };

    let tasks = rows.map_err(|e| AppError::Internal(e.to_string()))?;

    let result: Vec<serde_json::Value> = tasks
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "task_type": t.job_type,
                "status": map_status(&t.status),
                "priority": t.priority,
                "retry_count": t.attempts,
                "max_retries": t.max_attempts,
                "created_at": ts_to_string(Some(t.run_at)),
                "started_at": serde_json::Value::Null,
                "finished_at": ts_to_string(t.done_at),
                "last_error": t.last_result,
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
    let pool = &state.apalis_pool;

    let row = sqlx::query_as::<_, ApalisJobRow>(
        "SELECT id, job_type, status, attempts, max_attempts, run_at, done_at, last_result, priority
         FROM Jobs WHERE id = ?1"
    )
    .bind(&task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    match row {
        Some(t) => Ok(Json(serde_json::json!({
            "id": t.id,
            "task_type": t.job_type,
            "status": map_status(&t.status),
            "priority": t.priority,
            "retry_count": t.attempts,
            "max_retries": t.max_attempts,
            "created_at": ts_to_string(Some(t.run_at)),
            "started_at": serde_json::Value::Null,
            "finished_at": ts_to_string(t.done_at),
            "last_error": t.last_result,
        }))),
        None => Err(AppError::NotFound(format!("Task {} not found", task_id))),
    }
}

/// Row type for querying the Apalis Jobs table.
#[derive(sqlx::FromRow)]
struct ApalisJobRow {
    id: String,
    job_type: String,
    status: String,
    attempts: i32,
    max_attempts: i32,
    run_at: i64,
    done_at: Option<i64>,
    last_result: Option<String>,
    priority: i32,
}
