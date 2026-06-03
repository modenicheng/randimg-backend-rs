use crate::db::entities::apalis_job::{self, Entity as ApalisJob};
use crate::db::entities::task_dependency::{self, Entity as TaskDependency};
use sea_orm::*;
use sea_orm::Condition;
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A node in the task hierarchy tree, holding both the serialized job data
/// and any child nodes.
///
/// `job` is `serde_json::Value` (not `apalis_job::Model`) so that serialization
/// works correctly and the binary job-blob is exposed as an embedded `payload`
/// object rather than a raw byte array.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChildJobNode {
    /// Job information (id, job_type, status, attempts, …, payload).
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
/// The three boolean flags are computed in SQL via a recursive CTE.
/// The Rust side maps them to a derived status string.
#[derive(Debug, Clone)]
pub struct RootWithDerivedStatus {
    pub id: String,
    pub job_type: String,
    pub status: String,        // raw Apalis status (e.g. "Done")
    pub attempts: i32,
    pub max_attempts: i32,
    pub run_at: i64,           // unix timestamp (sqlite) — feature-gated below
    pub done_at: Option<i64>,
    pub last_result: Option<String>,
    pub priority: i32,
    pub job: Vec<u8>,          // serialized payload blob
    pub has_active: bool,
    pub has_failed: bool,
    pub has_completed: bool,
    /// `true` when at least one descendant has reached the terminal `Killed`
    /// state (retries exhausted or `AbortError`). Used to distinguish
    /// "transient failure, may still recover" from "definitively dead".
    pub has_killed_terminal: bool,
}

/// Map the descendant-status flags to a user-facing derived status string.
///
/// Resolution order:
/// 1. **Active wins**: any descendant still `Pending`/`Queued`/`Running` ⇒ `running`.
/// 2. **Terminal failure, all retries exhausted** (`has_failed && has_completed == false
///    && has_killed_terminal == has_failed`): every failed descendant is in the
///    terminal `Killed` state — no recovery possible ⇒ `killed`.
/// 3. **Mixed outcome** (some `Done`, some failed) ⇒ `partial_success`.
/// 4. **Transient failure** (failed descendants still in the `Failed` state with
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
        // Every failed descendant is terminal (Killed). The subtree is dead —
        // surface as `killed` so the UI matches the Apalis terminal state.
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

/// Map Apalis status strings to user-friendly labels.
pub fn status_label(status: &str) -> &'static str {
    match status {
        apalis_job::STATUS_PENDING => "pending",
        apalis_job::STATUS_QUEUED => "pending",
        apalis_job::STATUS_RUNNING => "running",
        apalis_job::STATUS_DONE   => "completed",
        apalis_job::STATUS_FAILED => "failed",
        apalis_job::STATUS_KILLED => "killed",
        _ => "unknown",
    }
}

/// Convert an `apalis_job::Model` into a JSON value suitable for API responses.
///
/// The binary `job` blob is deserialized and exposed as a nested `payload` field.
pub fn model_to_json(m: &apalis_job::Model) -> JsonValue {
    let payload = serde_json::from_slice::<JsonValue>(&m.job).ok();

    #[cfg(feature = "db-sqlite")]
    let run_at: Option<String> = chrono::DateTime::from_timestamp(m.run_at, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());

    #[cfg(feature = "db-sqlite")]
    let done_at: Option<String> = m.done_at.and_then(|ts| {
        chrono::DateTime::from_timestamp(ts, 0)
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
    });

    #[cfg(feature = "db-sqlite")]
    let last_result = m.last_result.clone();

    #[cfg(feature = "db-postgres")]
    let run_at: Option<String> = Some(m.run_at.format("%Y-%m-%dT%H:%M:%SZ").to_string());

    #[cfg(feature = "db-postgres")]
    let done_at: Option<String> = m.done_at.map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());

    #[cfg(feature = "db-postgres")]
    let last_result = m.last_result.as_ref().map(|v| v.to_string());

    serde_json::json!({
        "id":           m.id,
        "jobType":      m.job_type,
        "status":       status_label(&m.status),
        "rawStatus":    m.status,
        "attempts":     m.attempts,
        "maxAttempts":  m.max_attempts,
        "priority":     m.priority,
        "runAt":        run_at,
        "doneAt":       done_at,
        "lastResult":   last_result,
        "payload":      payload,
    })
}

// ---------------------------------------------------------------------------
// Roots
// ---------------------------------------------------------------------------

/// Build a reusable roots query: jobs that are NOT children in
/// `task_dependencies`.
#[cfg(feature = "db-sqlite")]
fn build_roots_query() -> Select<ApalisJob> {
    use sea_orm::QueryTrait;
    let mut q = ApalisJob::find();
    q = q.filter(
        apalis_job::Column::Id.not_in_subquery(
            TaskDependency::find()
                .select_only()
                .column(task_dependency::Column::ChildJobId)
                .into_query(),
        ),
    );
    q
}

#[cfg(feature = "db-postgres")]
fn build_roots_query() -> Select<ApalisJob> {
    use sea_orm::QueryTrait;
    let mut q = ApalisJob::find();
    q = q.filter(
        apalis_job::Column::Id.not_in_subquery(
            TaskDependency::find()
                .select_only()
                .column(task_dependency::Column::ChildJobId)
                .into_query(),
        ),
    );
    q
}

/// Return root tasks — jobs that have no parent in `task_dependencies`.
/// Supports optional `task_type` / `status` filters, plus `limit` / `offset`.
pub async fn list_roots(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    status: Option<&[&str]>,
    limit: u64,
    offset: u64,
) -> Result<Vec<apalis_job::Model>, DbErr> {
    let mut q = build_roots_query().order_by_desc(apalis_job::Column::RunAt);
    if let Some(tt) = task_type {
        q = q.filter(apalis_job::Column::JobType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(apalis_job::Column::Status.is_in(sts.iter().map(|s| *s)));
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
        q = q.filter(apalis_job::Column::JobType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(apalis_job::Column::Status.is_in(sts.iter().map(|s| *s)));
    }
    q.count(db).await
}

// ---------------------------------------------------------------------------
// Children → full job details (hierarchical)
// ---------------------------------------------------------------------------

/// Return all child jobs for `parent_id`, optionally filtered by type/status.
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

    let all_deps: Vec<String> = TaskDependency::find()
        .filter(task_dependency::Column::ParentJobId.eq(parent_id))
        .all(db)
        .await?
        .into_iter()
        .map(|d| d.child_job_id)
        .collect();

    if all_deps.is_empty() {
        return Ok(vec![]);
    }

    let mut q = ApalisJob::find().filter(apalis_job::Column::Id.is_in(all_deps));
    if let Some(tt) = task_type {
        q = q.filter(apalis_job::Column::JobType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(apalis_job::Column::Status.is_in(sts.iter().map(|s| *s)));
    }

    let jobs = q.order_by_desc(apalis_job::Column::RunAt).all(db).await?;

    // Recursively build children for each node, converting Model → JsonValue.
    let mut result = Vec::with_capacity(jobs.len());
    for job in jobs {
        let children = Box::pin(list_children(db, &job.id, task_type, status, max_depth - 1)).await?;
        result.push(ChildJobNode {
            job: model_to_json(&job),
            children,
        });
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Subtasks (flat, non-recursive — child jobs with full details)
// ---------------------------------------------------------------------------

/// Build a filtered query for child jobs of `parent_id`.
///
/// Returns `Ok(None)` when the parent has no children at all (short-circuit).
async fn filtered_subtask_query(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&[&str]>,
) -> Result<Option<sea_orm::Select<apalis_job::Entity>>, DbErr> {
    // Step 1: Get child_job_id values for this parent
    let child_ids: Vec<String> = TaskDependency::find()
        .filter(task_dependency::Column::ParentJobId.eq(parent_id))
        .all(db)
        .await?
        .into_iter()
        .map(|d| d.child_job_id)
        .collect();

    if child_ids.is_empty() {
        return Ok(None);
    }

    // Step 2: Build filtered query on the actual job records
    let mut q = ApalisJob::find().filter(apalis_job::Column::Id.is_in(child_ids));

    if let Some(tt) = task_type {
        q = q.filter(apalis_job::Column::JobType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(apalis_job::Column::Status.is_in(sts.iter().map(|s| *s)));
    }

    Ok(Some(q))
}

/// Return child jobs (one level) for a given parent, with optional type/status filters.
///
/// When `limit` and/or `offset` are provided the result is paged at the SQL level.
pub async fn list_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&[&str]>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<apalis_job::Model>, DbErr> {
    let Some(mut q) = filtered_subtask_query(db, parent_id, task_type, status).await? else {
        return Ok(vec![]);
    };

    q = q.order_by_desc(apalis_job::Column::RunAt);

    if let Some(l) = limit {
        q = q.limit(l);
    }
    if let Some(o) = offset {
        q = q.offset(o);
    }

    q.all(db).await
}

/// Count child jobs (one level) for a given parent, with optional type/status filters.
pub async fn count_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&[&str]>,
) -> Result<u64, DbErr> {
    let Some(q) = filtered_subtask_query(db, parent_id, task_type, status).await? else {
        return Ok(0);
    };

    q.count(db).await
}

// ---------------------------------------------------------------------------
// Interrupt (delete) pending subtasks
// ---------------------------------------------------------------------------

/// Delete all pending subtasks of `parent_id` (optionally filtered by job_type).
///
/// Also deletes the corresponding task_dependency rows.
/// Returns the list of deleted child_job_ids and the count of deleted dependency rows.
pub async fn interrupt_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    job_type: Option<&str>,
) -> Result<(Vec<String>, u64), DbErr> {
    // Find pending child jobs via task_dependencies → apalis_jobs
    let deps = TaskDependency::find()
        .filter(task_dependency::Column::ParentJobId.eq(parent_id))
        .all(db)
        .await?;

    if deps.is_empty() {
        return Ok((vec![], 0));
    }

    let child_ids: Vec<String> = deps.iter().map(|d| d.child_job_id.clone()).collect();

    // Filter by status=pending and optionally job_type
    let mut cond = Condition::all()
        .add(apalis_job::Column::Id.is_in(&child_ids))
        .add(
            apalis_job::Column::Status
                .is_in([apalis_job::STATUS_PENDING, apalis_job::STATUS_QUEUED]),
        );

    if let Some(tt) = job_type {
        cond = cond.add(apalis_job::Column::JobType.eq(tt));
    }

    let to_delete = ApalisJob::find().filter(cond).all(db).await?;
    let deleted_ids: Vec<String> = to_delete.iter().map(|j| j.id.clone()).collect();

    if deleted_ids.is_empty() {
        return Ok((vec![], 0));
    }

    // Delete dependency rows and job rows atomically
    let deleted_ids_clone = deleted_ids.clone();
    let dep_count = db.transaction::<_, u64, DbErr>(|txn| {
        Box::pin(async move {
            // Delete task_dependency rows that reference the deleted children
            let dep_deleted = TaskDependency::delete_many()
                .filter(task_dependency::Column::ChildJobId.is_in(&deleted_ids_clone))
                .exec(txn)
                .await?;

            // Delete the jobs themselves
            ApalisJob::delete_many()
                .filter(apalis_job::Column::Id.is_in(&deleted_ids_clone))
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

/// Delete all task_dependency rows where the given job is a parent.
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

/// Delete all task_dependency rows where the given job is a child.
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

    // Collect task_type as a bind parameter (placeholder added per-backend below).
    let has_task_type_filter = task_type.is_some();
    if let Some(tt) = task_type {
        bind_values.push(Value::from(tt));
    }

    // Collect crawl_type as a bind parameter (filters by payload JSON field).
    let has_crawl_type_filter = crawl_type.is_some();
    if let Some(ct) = crawl_type {
        bind_values.push(Value::from(ct));
    }

    match derived_status {
        // Priority: killed > failed > partial_success > running > completed > pending
        Some("killed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 1 \
                 AND COALESCE(rf.has_completed, 0) = 0 \
                 AND COALESCE(rf.has_killed_terminal, 0) = COALESCE(rf.has_failed, 0)",
            );
        }
        Some("failed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 1 \
                 AND COALESCE(rf.has_completed, 0) = 0 \
                 AND (COALESCE(rf.has_killed_terminal, 0) < COALESCE(rf.has_failed, 0) \
                      OR COALESCE(rf.has_completed, 0) = 1)",
            );
        }
        Some("partial_success") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_failed, 0) = 1 \
                 AND COALESCE(rf.has_completed, 0) = 1",
            );
        }
        Some("running") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 1 \
                 AND COALESCE(rf.has_failed, 0) = 0",
            );
        }
        Some("completed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 0 \
                 AND COALESCE(rf.has_completed, 0) = 1",
            );
        }
        Some("pending") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 0 \
                 AND COALESCE(rf.has_completed, 0) = 0",
            );
        }
        Some(_) => {
            extra_filters.push_str(" AND 1 = 0");
        }
        None => {}
    }

    #[cfg(feature = "db-sqlite")]
    let sql = {
        let mut filter = extra_filters.clone();
        if has_task_type_filter {
            filter.push_str(" AND r.job_type = ?");
        }
        if has_crawl_type_filter {
            filter.push_str(" AND json_extract(r.job, '$.crawl_type') = ?");
        }
        format!(
            r#"
            WITH RECURSIVE
                descendants AS (
                    SELECT td.parent_job_id AS root_id,
                           td.child_job_id AS descendant_id
                    FROM task_dependencies td
                    WHERE td.parent_job_id NOT IN (
                        SELECT child_job_id FROM task_dependencies
                    )
                    UNION ALL
                    SELECT d.root_id, td.child_job_id
                    FROM descendants d
                    JOIN task_dependencies td ON td.parent_job_id = d.descendant_id
                ),
                root_flags AS (
                    SELECT
                        d.root_id,
                        MAX(CASE WHEN j2.status IN ('Pending','Queued','Running') THEN 1 ELSE 0 END) AS has_active,
                        MAX(CASE WHEN j2.status IN ('Failed','Killed')              THEN 1 ELSE 0 END) AS has_failed,
                        MAX(CASE WHEN j2.status = 'Done'                            THEN 1 ELSE 0 END) AS has_completed,
                        MAX(CASE WHEN j2.status = 'Killed'                          THEN 1 ELSE 0 END) AS has_killed_terminal
                    FROM descendants d
                    JOIN Jobs j2 ON j2.id = d.descendant_id
                    GROUP BY d.root_id
                )
            SELECT
                r.id, r.job_type, r.status, r.attempts, r.max_attempts,
                r.run_at, r.done_at, r.last_result, r.priority, r.job,
                COALESCE(rf.has_active, 0)    AS has_active,
                COALESCE(rf.has_failed, 0)    AS has_failed,
                COALESCE(rf.has_completed, 0) AS has_completed,
                COALESCE(rf.has_killed_terminal, 0) AS has_killed_terminal
            FROM Jobs r
            LEFT JOIN root_flags rf ON rf.root_id = r.id
            WHERE r.id NOT IN (SELECT child_job_id FROM task_dependencies)
            {filter}
            ORDER BY r.run_at DESC
            LIMIT {limit} OFFSET {offset}
            "#
        )
    };

    #[cfg(feature = "db-postgres")]
    let sql = {
        let mut filter = extra_filters.clone();
        let mut next_bind = 1u32;
        if has_task_type_filter {
            filter.push_str(&format!(" AND r.job_type = ${}", next_bind));
            next_bind += 1;
        }
        if has_crawl_type_filter {
            filter.push_str(&format!(" AND (r.job::json->>'crawl_type')::int = ${}", next_bind));
        }
        format!(
            r#"
            WITH RECURSIVE
                descendants AS (
                    SELECT td.parent_job_id AS root_id,
                           td.child_job_id AS descendant_id
                    FROM task_dependencies td
                    WHERE td.parent_job_id NOT IN (
                        SELECT child_job_id FROM task_dependencies
                    )
                    UNION ALL
                    SELECT d.root_id, td.child_job_id
                    FROM descendants d
                    JOIN task_dependencies td ON td.parent_job_id = d.descendant_id
                ),
                root_flags AS (
                    SELECT
                        d.root_id,
                        BOOL_OR(j2.status IN ('Pending','Queued','Running')) AS has_active,
                        BOOL_OR(j2.status IN ('Failed','Killed'))            AS has_failed,
                        BOOL_OR(j2.status = 'Done')                          AS has_completed,
                        BOOL_OR(j2.status = 'Killed')                        AS has_killed_terminal
                    FROM descendants d
                    JOIN apalis.jobs j2 ON j2.id = d.descendant_id
                    GROUP BY d.root_id
                )
            SELECT
                r.id, r.job_type, r.status, r.attempts, r.max_attempts,
                r.run_at, r.done_at, r.last_result, r.priority, r.job,
                COALESCE(rf.has_active, false)    AS has_active,
                COALESCE(rf.has_failed, false)    AS has_failed,
                COALESCE(rf.has_completed, false) AS has_completed,
                COALESCE(rf.has_killed_terminal, false) AS has_killed_terminal
            FROM apalis.jobs r
            LEFT JOIN root_flags rf ON rf.root_id = r.id
            WHERE r.id NOT IN (SELECT child_job_id FROM task_dependencies)
            {filter}
            ORDER BY r.run_at DESC
            LIMIT {limit} OFFSET {offset}
            "#
        )
    };

    let stmt = Statement::from_sql_and_values(db.get_database_backend(), sql, bind_values);
    let rows = db.query_all(stmt).await?;

    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(RootWithDerivedStatus {
            id: row.try_get_by_index(0)?,
            job_type: row.try_get_by_index(1)?,
            status: row.try_get_by_index(2)?,
            attempts: row.try_get_by_index(3)?,
            max_attempts: row.try_get_by_index(4)?,
            run_at: row.try_get_by_index(5)?,
            done_at: row.try_get_by_index(6)?,
            last_result: row.try_get_by_index(7)?,
            priority: row.try_get_by_index(8)?,
            job: row.try_get_by_index(9)?,
            has_active: row.try_get_by_index::<i32>(10)? != 0,
            has_failed: row.try_get_by_index::<i32>(11)? != 0,
            has_completed: row.try_get_by_index::<i32>(12)? != 0,
            has_killed_terminal: row.try_get_by_index::<i32>(13)? != 0,
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

    // Collect task_type as a bind parameter (placeholder added per-backend below).
    let has_task_type_filter = task_type.is_some();
    if let Some(tt) = task_type {
        bind_values.push(Value::from(tt));
    }

    // Collect crawl_type as a bind parameter (filters by payload JSON field).
    let has_crawl_type_filter = crawl_type.is_some();
    if let Some(ct) = crawl_type {
        bind_values.push(Value::from(ct));
    }

    match derived_status {
        // Priority: killed > failed > partial_success > running > completed > pending
        Some("killed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 1 \
                 AND COALESCE(rf.has_completed, 0) = 0 \
                 AND COALESCE(rf.has_killed_terminal, 0) = COALESCE(rf.has_failed, 0)",
            );
        }
        Some("failed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 1 \
                 AND COALESCE(rf.has_completed, 0) = 0 \
                 AND (COALESCE(rf.has_killed_terminal, 0) < COALESCE(rf.has_failed, 0) \
                      OR COALESCE(rf.has_completed, 0) = 1)",
            );
        }
        Some("partial_success") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_failed, 0) = 1 \
                 AND COALESCE(rf.has_completed, 0) = 1",
            );
        }
        Some("running") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 1 \
                 AND COALESCE(rf.has_failed, 0) = 0",
            );
        }
        Some("completed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 0 \
                 AND COALESCE(rf.has_completed, 0) = 1",
            );
        }
        Some("pending") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 0 \
                 AND COALESCE(rf.has_completed, 0) = 0",
            );
        }
        Some(_) => {
            extra_filters.push_str(" AND 1 = 0");
        }
        None => {}
    }

    #[cfg(feature = "db-sqlite")]
    let sql = {
        let mut filter = extra_filters.clone();
        if has_task_type_filter {
            filter.push_str(" AND r.job_type = ?");
        }
        if has_crawl_type_filter {
            filter.push_str(" AND json_extract(r.job, '$.crawl_type') = ?");
        }
        format!(
            r#"
            WITH RECURSIVE
                descendants AS (
                    SELECT td.parent_job_id AS root_id,
                           td.child_job_id AS descendant_id
                    FROM task_dependencies td
                    WHERE td.parent_job_id NOT IN (
                        SELECT child_job_id FROM task_dependencies
                    )
                    UNION ALL
                    SELECT d.root_id, td.child_job_id
                    FROM descendants d
                    JOIN task_dependencies td ON td.parent_job_id = d.descendant_id
                ),
                root_flags AS (
                    SELECT
                        d.root_id,
                        MAX(CASE WHEN j2.status IN ('Pending','Queued','Running') THEN 1 ELSE 0 END) AS has_active,
                        MAX(CASE WHEN j2.status IN ('Failed','Killed')              THEN 1 ELSE 0 END) AS has_failed,
                        MAX(CASE WHEN j2.status = 'Done'                            THEN 1 ELSE 0 END) AS has_completed,
                        MAX(CASE WHEN j2.status = 'Killed'                          THEN 1 ELSE 0 END) AS has_killed_terminal
                    FROM descendants d
                    JOIN Jobs j2 ON j2.id = d.descendant_id
                    GROUP BY d.root_id
                )
            SELECT COUNT(*) AS cnt
            FROM Jobs r
            LEFT JOIN root_flags rf ON rf.root_id = r.id
            WHERE r.id NOT IN (SELECT child_job_id FROM task_dependencies)
            {filter}
            "#
        )
    };

    #[cfg(feature = "db-postgres")]
    let sql = {
        let mut filter = extra_filters.clone();
        let mut next_bind = 1u32;
        if has_task_type_filter {
            filter.push_str(&format!(" AND r.job_type = ${}", next_bind));
            next_bind += 1;
        }
        if has_crawl_type_filter {
            filter.push_str(&format!(" AND (r.job::json->>'crawl_type')::int = ${}", next_bind));
        }
        format!(
            r#"
            WITH RECURSIVE
                descendants AS (
                    SELECT td.parent_job_id AS root_id,
                           td.child_job_id AS descendant_id
                    FROM task_dependencies td
                    WHERE td.parent_job_id NOT IN (
                        SELECT child_job_id FROM task_dependencies
                    )
                    UNION ALL
                    SELECT d.root_id, td.child_job_id
                    FROM descendants d
                    JOIN task_dependencies td ON td.parent_job_id = d.descendant_id
                ),
                root_flags AS (
                    SELECT
                        d.root_id,
                        BOOL_OR(j2.status IN ('Pending','Queued','Running')) AS has_active,
                        BOOL_OR(j2.status IN ('Failed','Killed'))            AS has_failed,
                        BOOL_OR(j2.status = 'Done')                          AS has_completed,
                        BOOL_OR(j2.status = 'Killed')                        AS has_killed_terminal
                    FROM descendants d
                    JOIN apalis.jobs j2 ON j2.id = d.descendant_id
                    GROUP BY d.root_id
                )
            SELECT COUNT(*) AS cnt
            FROM apalis.jobs r
            LEFT JOIN root_flags rf ON rf.root_id = r.id
            WHERE r.id NOT IN (SELECT child_job_id FROM task_dependencies)
            {filter}
            "#
        )
    };

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
