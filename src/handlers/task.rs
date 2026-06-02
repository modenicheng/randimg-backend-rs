use axum::{
    Json, Router,
    extract::Path,
    extract::Query,
    extract::State,
    routing::{delete, get, post},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::auth::middleware::AuthUser;
use crate::db::entities::apalis_job;
use crate::db::query;
use crate::db::query::task_tree::ChildJobNode;
use crate::error::AppError;

/// Valid cleanup flag values.
const CLEAN_COMPLETED: &str = "completed";

/// Map short task type names (from frontend) to full Rust type paths stored in DB.
fn map_task_type(short: &str) -> &str {
    match short {
        "crawl" => "randimg_backend_rs::task_queue::jobs::CrawlJob",
        "download" => "randimg_backend_rs::task_queue::jobs::DownloadJob",
        "color-extract" | "color_extract" => "randimg_backend_rs::task_queue::jobs::ColorExtractJob",
        "upload" => "randimg_backend_rs::task_queue::jobs::UploadJob",
        "accessibility-check" | "accessibility_check" => "randimg_backend_rs::task_queue::jobs::AccessibilityCheckJob",
        "discover" => "randimg_backend_rs::task_queue::jobs::DiscoverJob",
        "refresh-pixiv-token" | "refresh_pixiv_token" => "randimg_backend_rs::task_queue::jobs::RefreshPixivTokenJob",
        other => other, // pass through if already a full path
    }
}
const CLEAN_FAILED: &str = "failed";
const CLEAN_CANCELLED: &str = "cancelled";
const CLEAN_KILLED: &str = "killed";
const CLEAN_PENDING: &str = "pending";
const CLEAN_RUNNING: &str = "running";
const CLEAN_ALL: &str = "all";

#[derive(Debug, Deserialize)]
pub struct CleanTasksRequest {
    /// List of status flags to clean: "completed", "failed", "cancelled", "killed", "pending", "running", "all".
    pub flags: Vec<String>,
    /// Optional: only clean tasks of this job type.
    #[serde(default)]
    pub task_type: Option<String>,
    /// Optional: filter crawl tasks by crawl_type (0=ranking, 1=user, 2=bookmarks).
    /// Only effective when task_type is "crawl".
    #[serde(default)]
    pub crawl_type: Option<i32>,
}

/// POST /tasks/clean — Bulk-delete tasks by status flags.
///
/// Accepts a JSON body with a `flags` array specifying which task states to purge.
/// Optionally filters by `task_type` to only clean tasks of a specific job type.
///
/// # Flags
///
/// | Flag        | Deletes                        | Side effects       |
/// |-------------|--------------------------------|--------------------|
/// | `completed` | Done tasks                     | None               |
/// | `failed`    | Failed tasks (retries exhausted)| None              |
/// | `cancelled` | Killed tasks (manually terminated)| None            |
/// | `pending`   | Pending tasks                  | None               |
/// | `running`   | Running tasks                  | Aborts all workers, then re-spawns |
/// | `all`       | All of the above               | Aborts all workers, then re-spawns |
///
/// Worker abort only happens when `task_type` is **not** set — aborting workers
/// for a filtered subset is not supported since workers are shared across types.
///
/// # Request
///
/// ```json
/// {
///   "flags": ["completed", "failed"],
///   "task_type": "crawl"       // optional
/// }
/// ```
///
/// # Response (200)
///
/// ```json
/// { "deleted": 42, "flags": ["completed", "failed"] }
/// ```
///
/// # Errors
///
/// - `400` — empty flags or invalid flag value
/// - `401` — missing or invalid auth token
async fn clean_tasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<CleanTasksRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let flags: Vec<&str> = body.flags.iter().map(|s| s.as_str()).collect();

    // Validate flags
    let valid_flags = [
        CLEAN_COMPLETED,
        CLEAN_FAILED,
        CLEAN_CANCELLED,
        CLEAN_KILLED,
        CLEAN_PENDING,
        CLEAN_RUNNING,
        CLEAN_ALL,
    ];
    if flags.is_empty() {
        return Err(AppError::BadRequest("At least one flag is required".into()));
    }
    for f in &flags {
        if !valid_flags.contains(f) {
            return Err(AppError::BadRequest(format!("Invalid flag: '{}'. Valid flags: {:?}", f, valid_flags)));
        }
    }

    let should_abort_workers = (flags.contains(&CLEAN_RUNNING) || flags.contains(&CLEAN_ALL))
        && body.task_type.is_none();

    // Resolve flags to Apalis status constants
    let mut statuses: Vec<&str> = Vec::new();
    for f in &flags {
        match *f {
            CLEAN_COMPLETED => statuses.push(apalis_job::STATUS_DONE),
            CLEAN_FAILED => statuses.push(apalis_job::STATUS_FAILED),
            CLEAN_CANCELLED => statuses.push(apalis_job::STATUS_KILLED),
            CLEAN_KILLED => statuses.push(apalis_job::STATUS_KILLED),
            CLEAN_PENDING => {
                statuses.push(apalis_job::STATUS_PENDING);
                statuses.push(apalis_job::STATUS_QUEUED);
            }
            CLEAN_RUNNING => statuses.push(apalis_job::STATUS_RUNNING),
            CLEAN_ALL => {
                statuses.push(apalis_job::STATUS_DONE);
                statuses.push(apalis_job::STATUS_FAILED);
                statuses.push(apalis_job::STATUS_KILLED);
                statuses.push(apalis_job::STATUS_PENDING);
                statuses.push(apalis_job::STATUS_QUEUED);
                statuses.push(apalis_job::STATUS_RUNNING);
            }
            _ => unreachable!("flag already validated"),
        }
    }
    statuses.sort();
    statuses.dedup();

    // Abort workers first if requested (only when no task_type filter)
    if should_abort_workers {
        let handles = {
            let mut guard = state.worker_handles.lock().await;
            std::mem::take(&mut *guard)
        };
        for h in &handles {
            h.abort();
        }
        tracing::info!("Aborted {} Apalis workers for cleanup", handles.len());
    }

    // Delete tasks — capture result without `?` so we can re-spawn workers even on error
    let mapped_type = body.task_type.as_deref().map(map_task_type);
    let delete_result = if let Some(ct) = body.crawl_type {
        // crawl_type filter: find matching crawl task IDs first, then delete them
        let ids = query::apalis_job::find_crawl_ids_by_type(&state.db, ct)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        if ids.is_empty() {
            Ok(0u64)
        } else {
            query::apalis_job::delete_by_statuses_and_ids(&state.db, &statuses, &ids)
                .await
        }
    } else {
        query::apalis_job::delete_by_statuses(
            &state.db,
            &statuses,
            mapped_type,
        )
        .await
    };

    // Re-spawn workers if they were aborted — MUST happen regardless of delete outcome
    if should_abort_workers {
        let new_handles = crate::spawn_workers(state.clone(), &state.apalis_pool, state.worker_runtime_handle.clone()).await;
        *state.worker_handles.lock().await = new_handles;
        tracing::info!("Re-spawned Apalis workers after cleanup");
    }

    // Propagate delete error AFTER workers have been re-spawned
    let deleted = delete_result?;

    Ok(Json(serde_json::json!({
        "deleted": deleted,
        "flags": body.flags,
    })))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // Static segments MUST precede dynamic ones so Axum matches them first.
        .route("/tasks", get(list_tasks))
        .route("/tasks/clean", post(clean_tasks))
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
        apalis_job::STATUS_QUEUED => "pending",
        apalis_job::STATUS_RUNNING => "running",
        apalis_job::STATUS_DONE => "completed",
        apalis_job::STATUS_FAILED => "failed",
        apalis_job::STATUS_KILLED => "killed",
        _ => "unknown",
    }
}

/// Map API status filter to Apalis status string(s).
fn unmap_status(status: &str) -> Vec<&'static str> {
    match status {
        "pending" | "queued" => vec![apalis_job::STATUS_PENDING, apalis_job::STATUS_QUEUED],
        "running" => vec![apalis_job::STATUS_RUNNING],
        "completed" => vec![apalis_job::STATUS_DONE],
        "failed" => vec![apalis_job::STATUS_FAILED],
        "killed" => vec![apalis_job::STATUS_KILLED],
        _ => vec![apalis_job::STATUS_PENDING, apalis_job::STATUS_QUEUED],
    }
}

// ---------------------------------------------------------------------------
// Timestamp formatting (feature-gated for SQLite i64 vs Postgres DateTime)
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlite")]
fn fmt_ts(ts: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

#[cfg(feature = "postgres")]
fn fmt_ts(ts: chrono::DateTime<chrono::Utc>) -> String {
    ts.format("%Y-%m-%dT%H:%M:%SZ").to_string()
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

/// GET /tasks — List background tasks with optional filters.
///
/// # Query Parameters
///
/// | Param       | Type   | Default | Description                        |
/// |-------------|--------|---------|------------------------------------|
/// | `task_type` | string | —       | Filter by job type (e.g. `crawl`, `download`, `color_extract`) |
/// | `status`    | string | —       | Filter by status: `pending`, `running`, `completed`, `failed` |
/// | `limit`     | int    | 50      | Page size (max 200)                |
/// | `offset`    | int    | 0       | Pagination offset                  |
///
/// # Response (200)
///
/// ```json
/// {
///   "tasks": [
///     {
///       "id": "01HZ...",
///       "job_type": "crawl",
///       "status": "completed",
///       "priority": 0,
///       "attempts": 1,
///       "max_attempts": 4,
///       "run_at": "2026-06-01 12:00:00",
///       "done_at": "2026-06-01 12:05:00",
///       "last_result": null,
///       "payload": { ... }
///     }
///   ],
///   "total": 128
/// }
/// ```
pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<ListTasksQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let limit = q.limit.unwrap_or(50).min(200);
    let offset = q.offset.unwrap_or(0);

    let db = &state.db;
    let mapped_status = q.status.as_deref().map(unmap_status);
    let mapped_type = q.task_type.as_deref().map(map_task_type);

    let (rows, total) = tokio::try_join!(
        query::apalis_job::list(db, mapped_type, mapped_status.clone(), limit, offset),
        query::apalis_job::count(db, mapped_type, mapped_status),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let result: Vec<serde_json::Value> = rows.iter().map(row_to_json).collect();

    Ok(Json(serde_json::json!({
        "tasks": result,
        "total": total,
    })))
}

/// GET /tasks/{id} — Get a single task by ID.
///
/// # Response (200)
///
/// Same shape as a single element in the `list_tasks` response array.
///
/// # Errors
///
/// - `404` — task not found
/// - `401` — missing or invalid auth token
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

/// DELETE /tasks/{id} — Delete (cancel) a task by ID.
///
/// Removes the task from the queue. If the task is currently running, the worker
/// will fail on its next poll (the job row is gone). Dependency rows in
/// `task_dependencies` are also cleaned up.
///
/// # Response (200)
///
/// ```json
/// { "message": "Task deleted" }
/// ```
///
/// # Errors
///
/// - `404` — task not found
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

/// DELETE /tasks/pending — Delete all pending tasks, optionally filtered by type.
///
/// Convenience shortcut equivalent to `POST /tasks/clean` with `flags: ["pending"]`.
/// Also cleans up orphaned `task_dependencies` rows.
///
/// # Query Parameters
///
/// | Param       | Type   | Description                           |
/// |-------------|--------|---------------------------------------|
/// | `task_type` | string | Optional: only delete pending tasks of this type |
///
/// # Response (200)
///
/// ```json
/// { "message": "Pending tasks deleted", "deleted": 15 }
/// ```
pub async fn delete_pending_tasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<DeletePendingQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mapped_type = q.task_type.as_deref().map(map_task_type);
    let deleted = query::apalis_job::delete_pending(&state.db, mapped_type)
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
    pub crawl_type: Option<i32>,
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
/// Returns only top-level tasks — jobs that are NOT children in any
/// `task_dependencies` relationship. Use this to see high-level crawl jobs
/// without the noise of their subtasks.
///
/// The `status` filter applies to the **derived** status (aggregated from the
/// entire descendant subtree), not the root's own Apalis status. This means a
/// root whose own status is "Done" but has failed children will appear as
/// `"partial_success"`.
///
/// # Query Parameters
///
/// Same as `GET /tasks`: `task_type`, `status`, `limit`, `offset`.
///
/// # Response (200)
///
/// ```json
/// { "tasks": [...job objects...], "total": 5 }
/// ```
pub async fn list_roots(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<RootsOrSubtasksQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let limit = q.limit.unwrap_or(50).min(200);
    let offset = q.offset.unwrap_or(0);
    let db = &state.db;
    let mapped_type = q.task_type.as_deref().map(map_task_type);

    // Filtering by derived status and pagination are pushed into the SQL CTE.
    // Run list + count in parallel.
    let (rows, total) = tokio::try_join!(
        query::task_tree::list_roots_derived(
            db,
            mapped_type,
            q.crawl_type,
            q.status.as_deref(),
            limit,
            offset,
        ),
        query::task_tree::count_roots_derived(
            db,
            mapped_type,
            q.crawl_type,
            q.status.as_deref(),
        ),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let items: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let payload = serde_json::from_slice::<serde_json::Value>(&r.job).ok();
            let derived = query::task_tree::derived_status_from_flags(
                r.has_active,
                r.has_failed,
                r.has_completed,
                r.has_killed_terminal,
            );

            // Two-phase derivation:
            //
            // 1. Dead-subtree short-circuit: if every failed descendant is in the
            //    terminal `Killed` state (no transient `Failed`), the subtree is
            //    unsalvageable — surface `killed` even if the root itself is still
            //    retrying. This matches the user's invariant: once all descendants
            //    are definitively done with no path to success, the root's outcome
            //    is `killed` regardless of its own Apalis status.
            // 2. Root-priority: when no such short-circuit applies, the root's own
            //    Apalis status wins. Only when the root has reached `completed` do
            //    we consider the descendant rollup. A root with no descendants
            //    naturally falls through to `completed`.
            let root_mapped = map_status(&r.status);
            let has_descendants = r.has_active || r.has_failed || r.has_completed;
            let subtree_dead_terminal = !r.has_active
                && r.has_failed
                && !r.has_completed
                && r.has_killed_terminal == r.has_failed;
            let effective = if subtree_dead_terminal {
                "killed"
            } else if root_mapped != "completed" {
                root_mapped
            } else if has_descendants {
                derived
            } else {
                "completed"
            };

            let run_at = fmt_ts(r.run_at);
            let done_at = r.done_at.and_then(fmt_ts);

            serde_json::json!({
                "id": r.id,
                "job_type": r.job_type,
                "status": effective,
                "raw_status": map_status(&r.status),
                "priority": r.priority,
                "attempts": r.attempts,
                "max_attempts": r.max_attempts,
                "run_at": run_at,
                "done_at": done_at,
                "last_result": r.last_result,
                "payload": payload,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "tasks": items,
        "total": total,
    })))
}

// ── GET /tasks/{id}/tree ────────────────────────────────────────────────────

/// GET /tasks/{id}/tree — Nested tree rooted at the given task ID.
///
/// Returns the full recursive hierarchy of subtasks. Each node contains the
/// job details and a `children` array of its own subtasks. Useful for
/// visualizing the entire crawl→download→extract pipeline of a single crawl job.
///
/// # Response (200)
///
/// ```json
/// {
///   "root_job_id": "01HZ...",
///   "children": [
///     {
///       "job": { "id": "...", "job_type": "download", "status": "completed", ... },
///       "children": [
///         { "job": { "id": "...", "job_type": "color_extract", ... }, "children": [] }
///       ]
///     }
///   ]
/// }
/// ```
///
/// Recursively flattens a `ChildJobNode` tree into a flat `Vec<JsonValue>`.
///
/// For each node, this function:
/// 1. Clones the job data
/// 2. Injects `parent_job_id` and `root_job_id` fields
/// 3. Pushes to the result vector
/// 4. Recursively processes children (depth-first, pre-order traversal)
///
/// The root node itself is NOT included — only its descendants. This matches
/// the nested mode behavior where the root is the URL parameter, not a child.
///
/// # Arguments
/// - `nodes`: The child nodes to flatten (output of `list_children`)
/// - `parent_id`: The job ID of the parent for the current level
/// - `root_id`: The root job ID (constant throughout recursion, from URL path)
fn flatten_tree(nodes: &[ChildJobNode], parent_id: &str, root_id: &str) -> Vec<serde_json::Value> {
    let mut result = Vec::new();
    for node in nodes {
        let mut job = node.job.clone();
        if let serde_json::Value::Object(ref mut map) = job {
            map.insert("parent_job_id".to_string(), serde_json::Value::String(parent_id.to_string()));
            map.insert("root_job_id".to_string(), serde_json::Value::String(root_id.to_string()));
        }
        result.push(job);
        let node_id = node.job.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        result.extend(flatten_tree(&node.children, node_id, root_id));
    }
    result
}

#[derive(Debug, Deserialize)]
pub struct TaskTreeQuery {
    pub flatten: Option<bool>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

/// GET /tasks/:id/tree
///
/// Returns the task tree rooted at `id`.
///
/// # Query Parameters
///
/// - `flatten` (optional, default: `false`)
///   - When `false` (default): Returns nested tree structure with `children` arrays.
///     Each node has `{ job: {...}, children: [...] }`. Frontend uses this for
///     tree UI with expand/collapse.
///   - When `true`: Returns flat list in `tasks` array. Each item includes
///     `parent_job_id` (direct parent) and `root_job_id` (the URL path root).
///     Frontend uses this for table/list UI with sorting and filtering.
///
/// - `limit` (optional): Maximum number of tasks to return in flattened mode.
///   Defaults to all tasks if not specified.
///
/// - `offset` (optional): Number of tasks to skip in flattened mode.
///   Defaults to 0 if not specified.
///
/// # Design Decision: Why flatten?
///
/// The nested tree format is natural for tree UI but painful for:
/// - Sorting/filtering across all descendants (requires recursive traversal in JS)
/// - Status rollup queries (need to flatten first anyway)
/// - Table display with columns (nested JSON doesn't map to rows)
///
/// The flat format trades tree structure for O(1) access to any task by index.
/// `parent_job_id` and `root_job_id` preserve the hierarchy information so the
/// frontend can reconstruct the tree if needed (build adjacency list from
/// parent_job_id).
///
/// # Why not a separate endpoint?
///
/// `?flatten` is a query parameter (not `/tasks/:id/tree/flat`) because:
/// - Same underlying data, different serialization
/// - Single route to maintain
/// - Consistent with REST conventions (representation varies by query params)
pub async fn get_task_tree(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
    Query(q): Query<TaskTreeQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tree = query::task_tree::list_children(
        &state.db,
        &task_id,
        None,
        None,
        20,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    if q.flatten.unwrap_or(false) {
        // FLATTENED MODE: Return all descendants as a flat array.
        // Each item has parent_job_id and root_job_id for hierarchy reconstruction.
        // Response shape: { "root_job_id": "...", "tasks": [...], "total": N }
        //
        // This differs from the default nested mode which returns:
        // { "root_job_id": "...", "children": [{ job, children }] }
        //
        // The different shapes are intentional — NOT a bug. See function doc.
        let all_tasks = flatten_tree(&tree, &task_id, &task_id);
        let total = all_tasks.len() as u64;

        let offset = q.offset.unwrap_or(0) as usize;
        let limit = q.limit.unwrap_or(total) as usize;
        let tasks: Vec<_> = all_tasks
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect();

        Ok(Json(serde_json::json!({
            "root_job_id": task_id,
            "tasks": tasks,
            "total": total,
        })))
    } else {
        // NESTED MODE: Return tree structure with children arrays.
        // Each node has { job: {...}, children: [...] } recursively.
        Ok(Json(serde_json::json!({
            "root_job_id": task_id,
            "children": tree,
        })))
    }
}

// ── GET /tasks/{id}/subtasks ────────────────────────────────────────────────

/// GET /tasks/{id}/subtasks — Flat list of direct children of the given task.
///
/// Unlike `/tasks/{id}/tree`, this returns only one level of children (not recursive).
/// Supports optional `task_type` and `status` filters, plus `limit`/`offset` pagination.
///
/// # Query Parameters
///
/// Same as `GET /tasks`: `task_type`, `status`, `limit`, `offset`.
///
/// # Response (200)
///
/// ```json
/// {
///   "parent_job_id": "01HZ...",
///   "subtasks": [...job objects...],
///   "total": 12
/// }
/// ```
pub async fn get_subtasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
    Query(q): Query<RootsOrSubtasksQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mapped_status: Option<Vec<&str>> = q.status.as_deref().map(|s| match s {
        "pending"   => vec![apalis_job::STATUS_PENDING, apalis_job::STATUS_QUEUED],
        "running"   => vec![apalis_job::STATUS_RUNNING],
        "completed" => vec![apalis_job::STATUS_DONE],
        "failed"    => vec![apalis_job::STATUS_FAILED],
        "killed"    => vec![apalis_job::STATUS_KILLED],
        other       => vec![other],
    });

    let limit = q.limit.map(|l| l as u64);
    let offset = q.offset.map(|o| o as u64);

    let mapped_type = q.task_type.as_deref().map(map_task_type);

    let total = query::task_tree::count_subtasks(
        &state.db,
        &task_id,
        mapped_type,
        mapped_status.as_deref(),
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let children = query::task_tree::list_subtasks(
        &state.db,
        &task_id,
        mapped_type,
        mapped_status.as_deref(),
        limit,
        offset,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let page: Vec<serde_json::Value> = children
        .into_iter()
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
/// This is the "cancel all subtasks" action. Only deletes children with
/// status `Pending` — running or completed subtasks are left untouched.
/// Also cleans up their `task_dependencies` rows.
///
/// # Query Parameters
///
/// | Param       | Type   | Description                                   |
/// |-------------|--------|-----------------------------------------------|
/// | `task_type` | string | Optional: only delete children of this type (e.g. `download`) |
///
/// # Response (200)
///
/// ```json
/// {
///   "parent_job_id": "01HZ...",
///   "cancelled": 8,
///   "child_ids": ["01HZ...", "01HZ...", ...]
/// }
/// ```
pub async fn interrupt_subtasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
    Query(q): Query<InterruptSubtasksQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mapped_type = q.task_type.as_deref().map(map_task_type);
    let (cancelled_ids, _) =
        query::task_tree::interrupt_subtasks(&state.db, &task_id, mapped_type)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "parent_job_id": task_id,
        "cancelled": cancelled_ids.len(),
        "child_ids": cancelled_ids,
    })))
}
