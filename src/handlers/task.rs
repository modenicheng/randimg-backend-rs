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

// ---------------------------------------------------------------------------
// SQL constants (feature-gated table name)
// ---------------------------------------------------------------------------

const SELECT_COLS: &str =
    "id, job_type, status, attempts, max_attempts, run_at, done_at, last_result, priority";

#[cfg(feature = "sqlite")]
const JOBS_TABLE: &str = "Jobs";
#[cfg(feature = "postgres")]
const JOBS_TABLE: &str = "apalis.jobs";

// ---------------------------------------------------------------------------
// ApalisJobRow (feature-gated field types)
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ApalisJobRow {
    id: String,
    job_type: String,
    status: String,
    attempts: i32,
    max_attempts: i32,
    #[cfg(feature = "sqlite")]
    run_at: i64,
    #[cfg(feature = "postgres")]
    run_at: chrono::DateTime<chrono::Utc>,
    #[cfg(feature = "sqlite")]
    done_at: Option<i64>,
    #[cfg(feature = "postgres")]
    done_at: Option<chrono::DateTime<chrono::Utc>>,
    #[cfg(feature = "sqlite")]
    last_result: Option<String>,
    #[cfg(feature = "postgres")]
    last_result: Option<serde_json::Value>,
    priority: i32,
}

// ---------------------------------------------------------------------------
// Timestamp / last_result formatting helpers (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlite")]
fn fmt_ts(ts: Option<i64>) -> Option<String> {
    ts.and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

#[cfg(feature = "postgres")]
fn fmt_ts(ts: Option<chrono::DateTime<chrono::Utc>>) -> Option<String> {
    ts.map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

#[cfg(feature = "sqlite")]
fn fmt_ts_req(ts: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

#[cfg(feature = "postgres")]
fn fmt_ts_req(ts: chrono::DateTime<chrono::Utc>) -> Option<String> {
    Some(ts.format("%Y-%m-%d %H:%M:%S").to_string())
}

#[cfg(feature = "sqlite")]
fn fmt_last_result(v: &Option<String>) -> Option<String> {
    v.clone()
}

#[cfg(feature = "postgres")]
fn fmt_last_result(v: &Option<serde_json::Value>) -> Option<String> {
    v.as_ref().map(|v| v.to_string())
}

// ---------------------------------------------------------------------------
// fetch_tasks helper (feature-gated SQL parameter syntax)
// ---------------------------------------------------------------------------

async fn fetch_tasks(
    pool: &crate::ApalisPool,
    task_type: Option<&str>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<ApalisJobRow>, sqlx::Error> {
    match (task_type, status) {
        (Some(tt), Some(st)) => {
            #[cfg(feature = "sqlite")]
            {
                sqlx::query_as::<_, ApalisJobRow>(&format!(
                    "{SELECT_COLS} FROM {JOBS_TABLE} WHERE job_type = ?1 AND status = ?2 ORDER BY run_at DESC LIMIT ?3 OFFSET ?4"
                ))
                .bind(tt)
                .bind(st)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
            }
            #[cfg(feature = "postgres")]
            {
                sqlx::query_as::<_, ApalisJobRow>(&format!(
                    "{SELECT_COLS} FROM {JOBS_TABLE} WHERE job_type = $1 AND status = $2 ORDER BY run_at DESC LIMIT $3 OFFSET $4"
                ))
                .bind(tt)
                .bind(st)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
            }
        }
        (Some(tt), None) => {
            #[cfg(feature = "sqlite")]
            {
                sqlx::query_as::<_, ApalisJobRow>(&format!(
                    "{SELECT_COLS} FROM {JOBS_TABLE} WHERE job_type = ?1 ORDER BY run_at DESC LIMIT ?2 OFFSET ?3"
                ))
                .bind(tt)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
            }
            #[cfg(feature = "postgres")]
            {
                sqlx::query_as::<_, ApalisJobRow>(&format!(
                    "{SELECT_COLS} FROM {JOBS_TABLE} WHERE job_type = $1 ORDER BY run_at DESC LIMIT $2 OFFSET $3"
                ))
                .bind(tt)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
            }
        }
        (None, Some(st)) => {
            #[cfg(feature = "sqlite")]
            {
                sqlx::query_as::<_, ApalisJobRow>(&format!(
                    "{SELECT_COLS} FROM {JOBS_TABLE} WHERE status = ?1 ORDER BY run_at DESC LIMIT ?2 OFFSET ?3"
                ))
                .bind(st)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
            }
            #[cfg(feature = "postgres")]
            {
                sqlx::query_as::<_, ApalisJobRow>(&format!(
                    "{SELECT_COLS} FROM {JOBS_TABLE} WHERE status = $1 ORDER BY run_at DESC LIMIT $2 OFFSET $3"
                ))
                .bind(st)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
            }
        }
        (None, None) => {
            #[cfg(feature = "sqlite")]
            {
                sqlx::query_as::<_, ApalisJobRow>(&format!(
                    "{SELECT_COLS} FROM {JOBS_TABLE} ORDER BY run_at DESC LIMIT ?1 OFFSET ?2"
                ))
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
            }
            #[cfg(feature = "postgres")]
            {
                sqlx::query_as::<_, ApalisJobRow>(&format!(
                    "{SELECT_COLS} FROM {JOBS_TABLE} ORDER BY run_at DESC LIMIT $1 OFFSET $2"
                ))
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// row_to_json helper (no cfg needed — uses feature-gated fmt_* functions)
// ---------------------------------------------------------------------------

fn row_to_json(t: &ApalisJobRow) -> serde_json::Value {
    serde_json::json!({
        "id": t.id,
        "task_type": t.job_type,
        "status": map_status(&t.status),
        "priority": t.priority,
        "retry_count": t.attempts,
        "max_retries": t.max_attempts,
        "created_at": fmt_ts_req(t.run_at),
        "started_at": serde_json::Value::Null,
        "finished_at": fmt_ts(t.done_at),
        "last_error": fmt_last_result(&t.last_result),
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
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let limit = q.limit.unwrap_or(50).min(200) as i64;
    let offset = q.offset.unwrap_or(0) as i64;

    let pool = &state.apalis_pool;

    let mapped_status = q.status.as_deref().map(unmap_status);

    let rows = fetch_tasks(
        pool,
        q.task_type.as_deref(),
        mapped_status.as_deref(),
        limit,
        offset,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let result: Vec<serde_json::Value> = rows.iter().map(row_to_json).collect();

    Ok(Json(result))
}

/// GET /tasks/:id — Get a single task by ID
pub async fn get_task(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = &state.apalis_pool;

    let row = {
        #[cfg(feature = "sqlite")]
        {
            sqlx::query_as::<_, ApalisJobRow>(&format!(
                "{SELECT_COLS} FROM {JOBS_TABLE} WHERE id = ?1"
            ))
            .bind(&task_id)
            .fetch_optional(pool)
            .await
        }
        #[cfg(feature = "postgres")]
        {
            sqlx::query_as::<_, ApalisJobRow>(&format!(
                "{SELECT_COLS} FROM {JOBS_TABLE} WHERE id = $1"
            ))
            .bind(&task_id)
            .fetch_optional(pool)
            .await
        }
    }
    .map_err(|e| AppError::Internal(e.to_string()))?;

    match row {
        Some(t) => Ok(Json(row_to_json(&t))),
        None => Err(AppError::NotFound(format!("Task {} not found", task_id))),
    }
}
