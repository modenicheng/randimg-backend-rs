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
        // Static segments MUST precede dynamic ones so Axum matches them first.
        .route("/tasks", get(list_tasks))
        .route("/tasks/roots", get(list_roots))
        .route("/tasks/pending", delete(delete_pending_tasks))
        .route(
            "/tasks/{task_id}",
            get(get_task).delete(delete_task),
        )
        .route("/tasks/{task_id}/tree", get(get_task_tree))
        .route(
            "/tasks/{task_id}/subtasks",
            get(get_subtasks).delete(interrupt_subtasks),
        )
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
        apalis_job::STATUS_KILLED => "failed",
        _ => "unknown",
    }
}

/// Map API status filter to Apalis status string(s).
fn unmap_status(status: &str) -> Vec<&'static str> {
    match status {
        "pending" => vec![apalis_job::STATUS_PENDING],
        "running" => vec![apalis_job::STATUS_RUNNING],
        "completed" => vec![apalis_job::STATUS_DONE],
        "failed" => vec![apalis_job::STATUS_FAILED, apalis_job::STATUS_KILLED],
        "killed" => vec![apalis_job::STATUS_KILLED],
        _ => vec![apalis_job::STATUS_PENDING],
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
        query::apalis_job::list(db, q.task_type.as_deref(), mapped_status.clone(), limit, offset),
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

// =========================================================================
// Tree endpoints — root tasks, subtasks, interrupt
// =========================================================================

#[derive(Deserialize)]
pub struct RootsOrSubtasksQuery {
    pub task_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

fn tree_row_to_json(t: &apalis_job::Model) -> serde_json::Value {
    row_to_json(t)
}

// ── GET /tasks/roots ────────────────────────────────────────────────────────

/// GET /tasks/roots — Root tasks (jobs without a parent), with filters and pagination.
///
/// Response:
/// { "tasks": [...], "total": int }
pub async fn list_roots(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<RootsOrSubtasksQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let limit = q.limit.unwrap_or(50).min(200);
    let offset = q.offset.unwrap_or(0);

    // Resolve the status filter: user sends lowercase, apalis stores "Pending" etc.
    let mapped_status = q.status.as_deref().map(|s| match s {
        "pending"   => apalis_job::STATUS_PENDING,
        "running"   => apalis_job::STATUS_RUNNING,
        "completed" => apalis_job::STATUS_DONE,
        "failed"    => apalis_job::STATUS_FAILED,
        other       => other, // pass through unknown as-is
    });

    let db = &state.db;

    let (rows, total) = tokio::try_join!(
        query::task_tree::list_roots(db, q.task_type.as_deref(), mapped_status, limit, offset),
        query::task_tree::count_roots(db, q.task_type.as_deref(), mapped_status),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let items: Vec<serde_json::Value> = rows.iter().map(tree_row_to_json).collect();

    Ok(Json(serde_json::json!({
        "tasks": items,
        "total": total,
    })))
}

// ── GET /tasks/{id}/tree ────────────────────────────────────────────────────

/// GET /tasks/{id}/tree — Nested tree rooted at the given task ID.
///
/// Returns the full recursive hierarchy, with job details at each node.
/// Response: { "root_job_id": "...", "children": [ { job: {...}, children: [...] }, ... ] }
pub async fn get_task_tree(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tree = query::task_tree::list_children(
        &state.db,
        &task_id,
        None, // task_type filter  – export as-is
        None, // status filter
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "root_job_id": task_id,
        "children": tree,
    })))
}

// ── GET /tasks/{id}/subtasks ────────────────────────────────────────────────

/// GET /tasks/{id}/subtasks — Flat list of direct children of the given task.
///
/// Response:
/// { "parent_job_id": "...", "subtasks": [...job objects...], "total": int }
pub async fn get_subtasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
    Query(q): Query<RootsOrSubtasksQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mapped_status = q.status.as_deref().map(|s| match s {
        "pending"   => apalis_job::STATUS_PENDING,
        "running"   => apalis_job::STATUS_RUNNING,
        "completed" => apalis_job::STATUS_DONE,
        "failed"    => apalis_job::STATUS_FAILED,
        other       => other,
    });

    let children = query::task_tree::list_subtasks(
        &state.db,
        &task_id,
        q.task_type.as_deref(),
        mapped_status,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let total = children.len() as u64;
    // Apply client-side limit/offset (subtask lists are typically small)
    let limit = q.limit.unwrap_or(100) as usize;
    let offset = q.offset.unwrap_or(0) as usize;

    let page: Vec<serde_json::Value> = children
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|j| tree_row_to_json(&j))
        .collect();

    Ok(Json(serde_json::json!({
        "parent_job_id": task_id,
        "subtasks": page,
        "total": total,
    })))
}

// ── DELETE /tasks/{id}/subtasks ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct InterruptSubtasksQuery {
    pub task_type: Option<String>,
}

/// DELETE /tasks/{id}/subtasks — Delete all **pending** children of the task.
///
/// This is the "cancel all subtasks" action.
/// The `task_type` query parameter optionally locks the operation to a specific
/// type (e.g. `download`, `color_extract`).
///
/// Response:
/// { "parent_job_id": "...", "cancelled": int, "child_ids": [...] }
pub async fn interrupt_subtasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
    Query(q): Query<InterruptSubtasksQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (cancelled_ids, _) =
        query::task_tree::interrupt_subtasks(&state.db, &task_id, q.task_type.as_deref())
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "parent_job_id": task_id,
        "cancelled": cancelled_ids.len(),
        "child_ids": cancelled_ids,
    })))
}
