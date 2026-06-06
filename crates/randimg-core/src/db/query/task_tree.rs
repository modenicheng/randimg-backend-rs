use crate::db::entities::task::{self, Entity as Task};
use crate::db::entities::task_enum::{TaskStatus, TaskType};
use chrono::{DateTime, Utc};
use sea_orm::Condition;
use sea_orm::sea_query::Expr;
use sea_orm::*;
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
    pub params: Option<serde_json::Value>,
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

/// Map task status enum to user-friendly labels.
pub(crate) fn status_label(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::Queued => "pending",
        TaskStatus::Running => "running",
        TaskStatus::Done => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Killed => "killed",
        TaskStatus::Dead => "dead",
    }
}

/// Convert a `task::Model` into a JSON value suitable for API responses.
pub fn model_to_json(m: &task::Model) -> JsonValue {
    let created_at = m.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let completed_at = m
        .completed_at
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());

    serde_json::json!({
        "id":           m.id,
        "taskType":     m.task_type.as_str(),
        "status":       status_label(&m.status),
        "rawStatus":    m.status.as_str(),
        "retryCount":   m.retry_count,
        "createdAt":    created_at,
        "completedAt":  completed_at,
        "errorMessage": m.error_message,
        "params":       m.params,
    })
}

// ---------------------------------------------------------------------------
// Children → full task details (hierarchical)
// ---------------------------------------------------------------------------

/// Return all child tasks for `parent_id`, optionally filtered by type/status/crawl_type.
///
/// NOTE: Filtering/pagination is applied to each level independently.
/// The entire subtree is still traversed recursively up to `max_depth` levels.
pub async fn list_children(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&TaskType>,
    status: Option<&[TaskStatus]>,
    crawl_type: Option<i32>,
    max_depth: u32,
) -> Result<Vec<ChildJobNode>, DbErr> {
    if max_depth == 0 {
        return Ok(vec![]);
    }

    let mut q = Task::find().filter(task::Column::ParentId.eq(parent_id));
    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt.clone()));
    }
    if let Some(sts) = status {
        q = q.filter(task::Column::Status.is_in(sts.iter().cloned()));
    }
    if let Some(ct) = crawl_type {
        q = q.filter(Expr::cust_with_values(
            "(params::json->>'crawl_type')::int = $1",
            [ct],
        ));
    }

    let tasks = q.order_by_desc(task::Column::CreatedAt).all(db).await?;

    // Recursively build children for each node, converting Model → JsonValue.
    let mut result = Vec::with_capacity(tasks.len());
    for t in tasks {
        let children = Box::pin(list_children(
            db,
            &t.id,
            task_type,
            status,
            crawl_type,
            max_depth - 1,
        ))
        .await?;
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
    task_type: Option<&TaskType>,
    status: Option<&[TaskStatus]>,
) -> Result<Option<sea_orm::Select<Task>>, DbErr> {
    let mut q = Task::find().filter(task::Column::ParentId.eq(parent_id));

    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt.clone()));
    }
    if let Some(sts) = status {
        q = q.filter(task::Column::Status.is_in(sts.iter().cloned()));
    }

    Ok(Some(q))
}

/// Return child tasks (one level) for a given parent, with optional type/status filters.
///
/// When `limit` and/or `offset` are provided the result is paged at the SQL level.
pub async fn list_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&TaskType>,
    status: Option<&[TaskStatus]>,
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
    task_type: Option<&TaskType>,
    status: Option<&[TaskStatus]>,
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
/// Returns the list of deleted task IDs, their fang_task_ids (for queue cleanup),
/// and the count of deleted tasks.
pub async fn interrupt_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&TaskType>,
) -> Result<(Vec<String>, Vec<String>, u64), DbErr> {
    let mut cond = Condition::all()
        .add(task::Column::ParentId.eq(parent_id))
        .add(task::Column::Status.is_in([TaskStatus::Pending, TaskStatus::Queued]));

    if let Some(tt) = task_type {
        cond = cond.add(task::Column::TaskType.eq(tt.clone()));
    }

    let to_delete = Task::find().filter(cond).all(db).await?;
    let deleted_ids: Vec<String> = to_delete.iter().map(|t| t.id.clone()).collect();
    let fang_task_ids: Vec<String> = to_delete
        .iter()
        .filter_map(|t| t.fang_task_id.clone())
        .collect();

    if deleted_ids.is_empty() {
        return Ok((vec![], vec![], 0));
    }

    let result = Task::delete_many()
        .filter(task::Column::Id.is_in(&deleted_ids))
        .exec(db)
        .await?;

    Ok((deleted_ids, fang_task_ids, result.rows_affected))
}

// ---------------------------------------------------------------------------
// Root listing with derived status (recursive CTE in SQL)
// ---------------------------------------------------------------------------

/// Build the shared recursive CTE and filter SQL used by both
/// `list_roots_derived` and `count_roots_derived`.
///
/// Returns `(cte_sql, filter_sql, bind_values)` where `cte_sql` is the full
/// `WITH RECURSIVE` CTE fragment (including the trailing `)`), `filter_sql`
/// is the combined `AND …` filter clause (may be empty), and `bind_values`
/// are the positional bind parameters matching `$1, $2, …` in the filter.
fn build_roots_cte_and_filters(
    task_type: Option<&TaskType>,
    crawl_type: Option<i32>,
    derived_status: Option<&str>,
) -> (String, String, Vec<Value>) {
    let mut extra_filters = String::new();
    let mut bind_values: Vec<Value> = Vec::new();

    // Collect task_type as a bind parameter.
    let has_task_type_filter = task_type.is_some();
    if let Some(tt) = task_type {
        bind_values.push(Value::from(tt.as_str()));
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
        filter.push_str(&format!(" AND t.task_type::text = ${}", next_bind));
        next_bind += 1;
    }
    if has_crawl_type_filter {
        filter.push_str(&format!(
            " AND (t.params::json->>'crawl_type')::int = ${}",
            next_bind
        ));
    }

    let cte_sql = "\
        WITH RECURSIVE\n\
            descendants AS (\n\
                SELECT t.id AS root_id,\n\
                       c.id AS descendant_id\n\
                FROM tasks t\n\
                JOIN tasks c ON c.parent_id = t.id\n\
                WHERE t.parent_id IS NULL\n\
                UNION ALL\n\
                SELECT d.root_id, c.id\n\
                FROM descendants d\n\
                JOIN tasks c ON c.parent_id = d.descendant_id\n\
            ),\n\
            root_flags AS (\n\
                SELECT\n\
                    d.root_id,\n\
                    BOOL_OR(t2.status::text IN ('pending','queued','running')) AS has_active,\n\
                    BOOL_OR(t2.status::text IN ('failed','killed'))            AS has_failed,\n\
                    BOOL_OR(t2.status::text = 'done')                          AS has_completed,\n\
                    BOOL_OR(t2.status::text = 'killed')                        AS has_killed_terminal\n\
                FROM descendants d\n\
                JOIN tasks t2 ON t2.id = d.descendant_id\n\
                GROUP BY d.root_id\n\
            )"
        .to_string();

    (cte_sql, filter, bind_values)
}

/// List root tasks with derived status flags computed from the descendant subtree.
///
/// Uses a recursive CTE to compute `has_active`, `has_failed`, `has_completed`
/// flags per root entirely in SQL. Supports filtering by `task_type` and
/// `derived_status`, plus `limit` / `offset` pagination.
pub async fn list_roots_derived(
    db: &DatabaseConnection,
    task_type: Option<&TaskType>,
    crawl_type: Option<i32>,
    derived_status: Option<&str>,
    limit: u64,
    offset: u64,
) -> Result<Vec<RootWithDerivedStatus>, DbErr> {
    let (cte, filter, bind_values) =
        build_roots_cte_and_filters(task_type, crawl_type, derived_status);

    let sql = format!(
        r#"
        {cte}
        SELECT
            t.id, t.task_type::text, t.status::text, t.root_id, t.crawler_id, t.image_id,
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
            params: row.try_get_by_index::<Option<serde_json::Value>>(11)?,
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
    task_type: Option<&TaskType>,
    crawl_type: Option<i32>,
    derived_status: Option<&str>,
) -> Result<u64, DbErr> {
    let (cte, filter, bind_values) =
        build_roots_cte_and_filters(task_type, crawl_type, derived_status);

    let sql = format!(
        r#"
        {cte}
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

// ---------------------------------------------------------------------------
// Flat descendants (root_id lookup)
// ---------------------------------------------------------------------------

/// Flat descendant row with parent/root IDs for hierarchy reconstruction.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FlatDescendant {
    pub id: String,
    pub task_type: String,
    pub status: String,
    pub parent_id: Option<String>,
    pub root_id: Option<String>,
    pub crawler_id: Option<i32>,
    pub image_id: Option<i32>,
    pub retry_count: i32,
    pub progress: f32,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub params: Option<JsonValue>,
}

/// List all descendants of `root_id` as a flat list, optionally filtered and paginated.
///
/// All filters (task_type, status, crawl_type) and pagination (limit/offset)
/// are pushed down to the database query.
pub async fn list_descendants_flat(
    db: &DatabaseConnection,
    root_id: &str,
    task_type: Option<&TaskType>,
    status: Option<&[TaskStatus]>,
    crawl_type: Option<i32>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<(Vec<FlatDescendant>, u64), DbErr> {
    let mut q = Task::find()
        .filter(task::Column::RootId.eq(root_id))
        .filter(task::Column::Id.ne(root_id));

    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt.clone()));
    }
    if let Some(sts) = status {
        q = q.filter(task::Column::Status.is_in(sts.iter().cloned()));
    }
    if let Some(ct) = crawl_type {
        q = q.filter(Expr::cust_with_values(
            "(params::json->>'crawl_type')::int = $1",
            [ct],
        ));
    }

    let total = q.clone().count(db).await?;

    let mut q = q.order_by_desc(task::Column::CreatedAt);
    if let Some(l) = limit {
        q = q.limit(l);
    }
    if let Some(o) = offset {
        q = q.offset(o);
    }
    let models = q.all(db).await?;

    let results: Vec<FlatDescendant> = models
        .into_iter()
        .map(|m| FlatDescendant {
            id: m.id,
            task_type: m.task_type.as_str().to_string(),
            status: m.status.as_str().to_string(),
            parent_id: m.parent_id,
            root_id: m.root_id,
            crawler_id: m.crawler_id,
            image_id: m.image_id,
            retry_count: m.retry_count,
            progress: m.progress,
            priority: m.priority,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
            completed_at: m.completed_at.map(|dt| dt.with_timezone(&Utc)),
            error_message: m.error_message,
            params: m.params,
        })
        .collect();

    Ok((results, total))
}
