use crate::db::entities::task::{self, Entity as Task};
use crate::db::entities::task_dependency::{self, Entity as TaskDependency};
use chrono::{DateTime, Utc};
use sea_orm::*;
use sea_orm::Condition;
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A node in the task hierarchy tree, holding both the serialized task data
/// and any child nodes.
///
/// `job` is `serde_json::Value` (not `task::Model`) so that serialization
/// works correctly.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChildJobNode {
    /// Task information (id, task_type, status, retry_count, …, params).
    pub job: JsonValue,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ChildJobNode>,
}

/// Summary of child statuses for a root task.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ChildSummary {
    pub total: u64,
    pub pending: u64,
    pub running: u64,
    pub failed: u64,
    pub completed: u64,
    pub killed: u64,
}

/// Root task row with derived status flags computed from the descendant subtree.
///
/// The four boolean flags are computed in SQL via a recursive CTE.
/// The Rust side maps them to a derived status string.
#[derive(Debug, Clone)]
pub struct RootWithDerivedStatus {
    pub id: String,
    pub task_type: String,
    pub status: String,
    pub root_id: Option<String>,
    pub crawler_id: Option<i32>,
    pub image_id: Option<i32>,
    pub retry_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub params: Option<String>,
    pub has_active: bool,
    pub has_failed: bool,
    pub has_completed: bool,
    /// `true` when at least one descendant has reached the terminal `killed`
    /// state (retries exhausted or `AbortError`). Used to distinguish
    /// "transient failure, may still recover" from "definitively dead".
    pub has_killed_terminal: bool,
}

/// Map the descendant-status flags to a user-facing derived status string.
///
/// Resolution order:
/// 1. **Active wins**: any descendant still `pending`/`queued`/`running` ⇒ `running`.
/// 2. **Terminal failure, all retries exhausted** (`has_failed && has_completed == false
///    && has_killed_terminal == has_failed`): every failed descendant is in the
///    terminal `killed` state — no recovery possible ⇒ `killed`.
/// 3. **Mixed outcome** (some `done`, some failed) ⇒ `partial_success`.
/// 4. **Transient failure** (failed descendants still in the `failed` state with
///    retries remaining) ⇒ `failed`.
/// 5. **All done** ⇒ `completed`.
/// 6. **Empty subtree** ⇒ `pending` (the rollup is not consulted in this case by
///    the caller, but we degrade safely here).
///
/// This rollup is only consulted after the root itself has reached `completed`
/// (see `list_roots::effective`), with the exception of rule 2, which short-circuits
/// the root-priority check because a fully-killed subtree means the root cannot
/// produce a useful result either.
pub fn derived_status_from_flags(
    has_active: bool,
    has_failed: bool,
    has_completed: bool,
    has_killed_terminal: bool,
) -> &'static str {
    if has_active {
        "running"
    } else if has_failed && !has_completed && has_killed_terminal == has_failed {
        "killed"
    } else if has_failed && has_completed {
        "partial_success"
    } else if has_failed {
        "failed"
    } else if has_completed {
        "completed"
    } else {
        "pending"
    }
}

// ---------------------------------------------------------------------------
// Model → JsonValue helper (avoids needing Serialize on Model)
// ---------------------------------------------------------------------------

/// Map task status strings to user-friendly labels.
pub fn status_label(status: &str) -> &'static str {
    match status {
        task::STATUS_PENDING => "pending",
        task::STATUS_QUEUED => "pending",
        task::STATUS_RUNNING => "running",
        task::STATUS_DONE => "completed",
        task::STATUS_FAILED => "failed",
        task::STATUS_KILLED => "killed",
        _ => "unknown",
    }
}

/// Convert a `task::Model` into a JSON value suitable for API responses.
pub fn model_to_json(m: &task::Model) -> JsonValue {
    let params: Option<JsonValue> = m.params.as_ref().and_then(|p| serde_json::from_str(p).ok());

    let created_at = m.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let completed_at = m.completed_at.map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());

    serde_json::json!({
        "id":           m.id,
        "taskType":     m.task_type,
        "status":       status_label(&m.status),
        "rawStatus":    m.status,
        "retryCount":   m.retry_count,
        "createdAt":    created_at,
        "completedAt":  completed_at,
        "errorMessage": m.error_message,
        "params":       params,
    })
}

// ---------------------------------------------------------------------------
// Roots
// ---------------------------------------------------------------------------

/// Build a reusable roots query: tasks that have no parent (`parent_id IS NULL`).
fn build_roots_query() -> Select<Task> {
    Task::find().filter(task::Column::ParentId.is_null())
}

/// Return root tasks — tasks that have `parent_id IS NULL`.
/// Supports optional `task_type` / `status` filters, plus `limit` / `offset`.
pub async fn list_roots(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    status: Option<&[&str]>,
    limit: u64,
    offset: u64,
) -> Result<Vec<task::Model>, DbErr> {
    let mut q = build_roots_query().order_by_desc(task::Column::CreatedAt);
    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(task::Column::Status.is_in(sts.iter().map(|s| *s)));
    }
    q.limit(limit).offset(offset).all(db).await
}

/// Count root tasks (with optional filters).
pub async fn count_roots(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    status: Option<&[&str]>,
) -> Result<u64, DbErr> {
    let mut q = build_roots_query();
    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(task::Column::Status.is_in(sts.iter().map(|s| *s)));
    }
    q.count(db).await
}

// ---------------------------------------------------------------------------
// Children → full task details (hierarchical)
// ---------------------------------------------------------------------------

/// Return all child tasks for `parent_id`, optionally filtered by type/status.
///
/// NOTE: Filtering/pagination is applied to each level independently.
/// The entire subtree is still traversed recursively up to `max_depth` levels.
pub async fn list_children(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&[&str]>,
    max_depth: u32,
) -> Result<Vec<ChildJobNode>, DbErr> {
    if max_depth == 0 {
        return Ok(vec![]);
    }

    let mut q = Task::find().filter(task::Column::ParentId.eq(parent_id));
    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(task::Column::Status.is_in(sts.iter().map(|s| *s)));
    }

    let tasks = q.order_by_desc(task::Column::CreatedAt).all(db).await?;

    // Recursively build children for each node, converting Model → JsonValue.
    let mut result = Vec::with_capacity(tasks.len());
    for t in tasks {
        let children = Box::pin(list_children(db, &t.id, task_type, status, max_depth - 1)).await?;
        result.push(ChildJobNode {
            job: model_to_json(&t),
            children,
        });
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Subtasks (flat, non-recursive — child tasks with full details)
// ---------------------------------------------------------------------------

/// Build a filtered query for child tasks of `parent_id`.
///
/// Returns `Ok(None)` when the parent has no children at all (short-circuit).
async fn filtered_subtask_query(
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&[&str]>,
) -> Result<Option<sea_orm::Select<Task>>, DbErr> {
    let mut q = Task::find().filter(task::Column::ParentId.eq(parent_id));

    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(task::Column::Status.is_in(sts.iter().map(|s| *s)));
    }

    Ok(Some(q))
}

/// Return child tasks (one level) for a given parent, with optional type/status filters.
///
/// When `limit` and/or `offset` are provided the result is paged at the SQL level.
pub async fn list_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&[&str]>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<task::Model>, DbErr> {
    let Some(mut q) = filtered_subtask_query(parent_id, task_type, status).await? else {
        return Ok(vec![]);
    };

    q = q.order_by_desc(task::Column::CreatedAt);

    if let Some(l) = limit {
        q = q.limit(l);
    }
    if let Some(o) = offset {
        q = q.offset(o);
    }

    q.all(db).await
}

/// Count child tasks (one level) for a given parent, with optional type/status filters.
pub async fn count_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&[&str]>,
) -> Result<u64, DbErr> {
    let Some(q) = filtered_subtask_query(parent_id, task_type, status).await? else {
        return Ok(0);
    };

    q.count(db).await
}

// ---------------------------------------------------------------------------
// Interrupt (delete) pending subtasks
// ---------------------------------------------------------------------------

/// Delete all pending subtasks of `parent_id` (optionally filtered by task_type).
///
/// Also deletes the corresponding task_dependency rows.
/// Returns the list of deleted task IDs and the count of deleted dependency rows.
pub async fn interrupt_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
) -> Result<(Vec<String>, u64), DbErr> {
    // Find pending child tasks
    let mut cond = Condition::all()
        .add(task::Column::ParentId.eq(parent_id))
        .add(
            task::Column::Status
                .is_in([task::STATUS_PENDING, task::STATUS_QUEUED]),
        );

    if let Some(tt) = task_type {
        cond = cond.add(task::Column::TaskType.eq(tt));
    }

    let to_delete = Task::find().filter(cond).all(db).await?;
    let deleted_ids: Vec<String> = to_delete.iter().map(|t| t.id.clone()).collect();

    if deleted_ids.is_empty() {
        return Ok((vec![], 0));
    }

    // Delete dependency rows and task rows atomically
    let deleted_ids_clone = deleted_ids.clone();
    let dep_count = db.transaction::<_, u64, DbErr>(|txn| {
        Box::pin(async move {
            // Delete task_dependency rows that reference the deleted children
            let dep_deleted = TaskDependency::delete_many()
                .filter(task_dependency::Column::ChildJobId.is_in(&deleted_ids_clone))
                .exec(txn)
                .await?;

            // Delete the tasks themselves
            Task::delete_many()
                .filter(task::Column::Id.is_in(&deleted_ids_clone))
                .exec(txn)
                .await?;

            Ok(dep_deleted.rows_affected)
        })
    })
    .await
    .map_err(|e| match e {
        TransactionError::Connection(e) => e,
        TransactionError::Transaction(e) => e,
    })?;

    Ok((deleted_ids, dep_count))
}

/// Delete all task_dependency rows where the given task is a parent.
pub async fn clear_dependencies_for_parent(
    db: &DatabaseConnection,
    parent_id: &str,
) -> Result<u64, DbErr> {
    let result = TaskDependency::delete_many()
        .filter(task_dependency::Column::ParentJobId.eq(parent_id))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

/// Delete all task_dependency rows where the given task is a child.
pub async fn clear_dependencies_for_child(
    db: &DatabaseConnection,
    child_id: &str,
) -> Result<u64, DbErr> {
    let result = TaskDependency::delete_many()
        .filter(task_dependency::Column::ChildJobId.eq(child_id))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

// ---------------------------------------------------------------------------
// Root listing with derived status (recursive CTE in SQL)
// ---------------------------------------------------------------------------

/// List root tasks with derived status flags computed from the descendant subtree.
///
/// Uses a recursive CTE to compute `has_active`, `has_failed`, `has_completed`
/// flags per root entirely in SQL. Supports filtering by `task_type` and
/// `derived_status`, plus `limit` / `offset` pagination.
pub async fn list_roots_derived(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    crawl_type: Option<i32>,
    derived_status: Option<&str>,
    limit: u64,
    offset: u64,
) -> Result<Vec<RootWithDerivedStatus>, DbErr> {
    let mut extra_filters = String::new();
    let mut bind_values: Vec<Value> = Vec::new();

    // Collect task_type as a bind parameter.
    let has_task_type_filter = task_type.is_some();
    if let Some(tt) = task_type {
        bind_values.push(Value::from(tt));
    }

    // Collect crawl_type as a bind parameter (filters by params JSON field).
    let has_crawl_type_filter = crawl_type.is_some();
    if let Some(ct) = crawl_type {
        bind_values.push(Value::from(ct));
    }

    match derived_status {
        Some("killed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, false) = false \
                 AND COALESCE(rf.has_failed, false) = true \
                 AND COALESCE(rf.has_completed, false) = false \
                 AND COALESCE(rf.has_killed_terminal, false) = COALESCE(rf.has_failed, false)",
            );
        }
        Some("failed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, false) = false \
                 AND COALESCE(rf.has_failed, false) = true \
                 AND COALESCE(rf.has_completed, false) = false \
                 AND (COALESCE(rf.has_killed_terminal, false) = false \
                      OR COALESCE(rf.has_completed, false) = true)",
            );
        }
        Some("partial_success") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_failed, false) = true \
                 AND COALESCE(rf.has_completed, false) = true",
            );
        }
        Some("running") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, false) = true \
                 AND COALESCE(rf.has_failed, false) = false",
            );
        }
        Some("completed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, false) = false \
                 AND COALESCE(rf.has_failed, false) = false \
                 AND COALESCE(rf.has_completed, false) = true",
            );
        }
        Some("pending") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, false) = false \
                 AND COALESCE(rf.has_failed, false) = false \
                 AND COALESCE(rf.has_completed, false) = false",
            );
        }
        Some(_) => {
            extra_filters.push_str(" AND false");
        }
        None => {}
    }

    let mut filter = extra_filters;
    let mut next_bind = (bind_values.len() + 1) as u32;
    if has_task_type_filter {
        filter.push_str(&format!(" AND t.task_type = ${}", next_bind));
        next_bind += 1;
    }
    if has_crawl_type_filter {
        filter.push_str(&format!(" AND (t.params::json->>'crawl_type')::int = ${}", next_bind));
    }

    let sql = format!(
        r#"
        WITH RECURSIVE
            descendants AS (
                SELECT t.id AS root_id,
                       c.id AS descendant_id
                FROM tasks t
                JOIN tasks c ON c.parent_id = t.id
                WHERE t.parent_id IS NULL
                UNION ALL
                SELECT d.root_id, c.id
                FROM descendants d
                JOIN tasks c ON c.parent_id = d.descendant_id
            ),
            root_flags AS (
                SELECT
                    d.root_id,
                    BOOL_OR(t2.status IN ('pending','queued','running')) AS has_active,
                    BOOL_OR(t2.status IN ('failed','killed'))            AS has_failed,
                    BOOL_OR(t2.status = 'done')                          AS has_completed,
                    BOOL_OR(t2.status = 'killed')                        AS has_killed_terminal
                FROM descendants d
                JOIN tasks t2 ON t2.id = d.descendant_id
                GROUP BY d.root_id
            )
        SELECT
            t.id, t.task_type, t.status, t.root_id, t.crawler_id, t.image_id,
            t.retry_count, t.created_at, t.updated_at, t.completed_at,
            t.error_message, t.params,
            COALESCE(rf.has_active, false)           AS has_active,
            COALESCE(rf.has_failed, false)           AS has_failed,
            COALESCE(rf.has_completed, false)        AS has_completed,
            COALESCE(rf.has_killed_terminal, false)  AS has_killed_terminal
        FROM tasks t
        LEFT JOIN root_flags rf ON rf.root_id = t.id
        WHERE t.parent_id IS NULL
        {filter}
        ORDER BY t.created_at DESC
        LIMIT {limit} OFFSET {offset}
        "#
    );

    let stmt = Statement::from_sql_and_values(db.get_database_backend(), sql, bind_values);
    let rows = db.query_all(stmt).await?;

    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(RootWithDerivedStatus {
            id: row.try_get_by_index(0)?,
            task_type: row.try_get_by_index(1)?,
            status: row.try_get_by_index(2)?,
            root_id: row.try_get_by_index(3)?,
            crawler_id: row.try_get_by_index(4)?,
            image_id: row.try_get_by_index(5)?,
            retry_count: row.try_get_by_index(6)?,
            created_at: row.try_get_by_index(7)?,
            updated_at: row.try_get_by_index(8)?,
            completed_at: row.try_get_by_index(9)?,
            error_message: row.try_get_by_index(10)?,
            params: row.try_get_by_index(11)?,
            has_active: row.try_get_by_index::<bool>(12)?,
            has_failed: row.try_get_by_index::<bool>(13)?,
            has_completed: row.try_get_by_index::<bool>(14)?,
            has_killed_terminal: row.try_get_by_index::<bool>(15)?,
        });
    }

    Ok(results)
}

/// Count root tasks with derived status filters applied.
///
/// Uses the same recursive CTE as `list_roots_derived` but returns only the
/// count, which is more efficient for pagination metadata.
pub async fn count_roots_derived(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    crawl_type: Option<i32>,
    derived_status: Option<&str>,
) -> Result<u64, DbErr> {
    let mut extra_filters = String::new();
    let mut bind_values: Vec<Value> = Vec::new();

    // Collect task_type as a bind parameter.
    let has_task_type_filter = task_type.is_some();
    if let Some(tt) = task_type {
        bind_values.push(Value::from(tt));
    }

    // Collect crawl_type as a bind parameter (filters by params JSON field).
    let has_crawl_type_filter = crawl_type.is_some();
    if let Some(ct) = crawl_type {
        bind_values.push(Value::from(ct));
    }

    match derived_status {
        Some("killed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, false) = false \
                 AND COALESCE(rf.has_failed, false) = true \
                 AND COALESCE(rf.has_completed, false) = false \
                 AND COALESCE(rf.has_killed_terminal, false) = COALESCE(rf.has_failed, false)",
            );
        }
        Some("failed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, false) = false \
                 AND COALESCE(rf.has_failed, false) = true \
                 AND COALESCE(rf.has_completed, false) = false \
                 AND (COALESCE(rf.has_killed_terminal, false) = false \
                      OR COALESCE(rf.has_completed, false) = true)",
            );
        }
        Some("partial_success") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_failed, false) = true \
                 AND COALESCE(rf.has_completed, false) = true",
            );
        }
        Some("running") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, false) = true \
                 AND COALESCE(rf.has_failed, false) = false",
            );
        }
        Some("completed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, false) = false \
                 AND COALESCE(rf.has_failed, false) = false \
                 AND COALESCE(rf.has_completed, false) = true",
            );
        }
        Some("pending") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, false) = false \
                 AND COALESCE(rf.has_failed, false) = false \
                 AND COALESCE(rf.has_completed, false) = false",
            );
        }
        Some(_) => {
            extra_filters.push_str(" AND false");
        }
        None => {}
    }

    let mut filter = extra_filters;
    let mut next_bind = (bind_values.len() + 1) as u32;
    if has_task_type_filter {
        filter.push_str(&format!(" AND t.task_type = ${}", next_bind));
        next_bind += 1;
    }
    if has_crawl_type_filter {
        filter.push_str(&format!(" AND (t.params::json->>'crawl_type')::int = ${}", next_bind));
    }

    let sql = format!(
        r#"
        WITH RECURSIVE
            descendants AS (
                SELECT t.id AS root_id,
                       c.id AS descendant_id
                FROM tasks t
                JOIN tasks c ON c.parent_id = t.id
                WHERE t.parent_id IS NULL
                UNION ALL
                SELECT d.root_id, c.id
                FROM descendants d
                JOIN tasks c ON c.parent_id = d.descendant_id
            ),
            root_flags AS (
                SELECT
                    d.root_id,
                    BOOL_OR(t2.status IN ('pending','queued','running')) AS has_active,
                    BOOL_OR(t2.status IN ('failed','killed'))            AS has_failed,
                    BOOL_OR(t2.status = 'done')                          AS has_completed,
                    BOOL_OR(t2.status = 'killed')                        AS has_killed_terminal
                FROM descendants d
                JOIN tasks t2 ON t2.id = d.descendant_id
                GROUP BY d.root_id
            )
        SELECT COUNT(*) AS cnt
        FROM tasks t
        LEFT JOIN root_flags rf ON rf.root_id = t.id
        WHERE t.parent_id IS NULL
        {filter}
        "#
    );

    let stmt = Statement::from_sql_and_values(db.get_database_backend(), sql, bind_values);
    let row = db.query_one(stmt).await?;

    match row {
        Some(r) => {
            let cnt: i64 = r.try_get_by_index(0)?;
            Ok(cnt as u64)
        }
        None => Ok(0),
    }
}
