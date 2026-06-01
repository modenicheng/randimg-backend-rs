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

// ---------------------------------------------------------------------------
// Model → JsonValue helper (avoids needing Serialize on Model)
// ---------------------------------------------------------------------------

/// Map Apalis status strings to user-friendly labels.
pub fn status_label(status: &str) -> &'static str {
    match status {
        apalis_job::STATUS_PENDING => "pending",
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

    #[cfg(feature = "sqlite")]
    let run_at: Option<String> = chrono::DateTime::from_timestamp(m.run_at, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());

    #[cfg(feature = "sqlite")]
    let done_at: Option<String> = m.done_at.and_then(|ts| {
        chrono::DateTime::from_timestamp(ts, 0)
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
    });

    #[cfg(feature = "sqlite")]
    let last_result = m.last_result.clone();

    #[cfg(feature = "postgres")]
    let run_at: Option<String> = Some(m.run_at.format("%Y-%m-%dT%H:%M:%SZ").to_string());

    #[cfg(feature = "postgres")]
    let done_at: Option<String> = m.done_at.map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());

    #[cfg(feature = "postgres")]
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
#[cfg(feature = "sqlite")]
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

#[cfg(feature = "postgres")]
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
    status: Option<&str>,
    limit: u64,
    offset: u64,
) -> Result<Vec<apalis_job::Model>, DbErr> {
    let mut q = build_roots_query().order_by_desc(apalis_job::Column::RunAt);
    if let Some(tt) = task_type {
        q = q.filter(apalis_job::Column::JobType.eq(tt));
    }
    if let Some(st) = status {
        q = q.filter(apalis_job::Column::Status.eq(st));
    }
    q.limit(limit).offset(offset).all(db).await
}

/// Count root tasks (with optional filters).
pub async fn count_roots(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    status: Option<&str>,
) -> Result<u64, DbErr> {
    let mut q = build_roots_query();
    if let Some(tt) = task_type {
        q = q.filter(apalis_job::Column::JobType.eq(tt));
    }
    if let Some(st) = status {
        q = q.filter(apalis_job::Column::Status.eq(st));
    }
    q.count(db).await
}

// ---------------------------------------------------------------------------
// Children → full job details (hierarchical)
// ---------------------------------------------------------------------------

/// Return all child jobs for `parent_id`, optionally filtered by type/status.
///
/// NOTE: Filtering/pagination is applied to each level independently.
/// The entire subtree is still traversed recursively.
pub async fn list_children(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<ChildJobNode>, DbErr> {
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
    if let Some(st) = status {
        q = q.filter(apalis_job::Column::Status.eq(st));
    }

    let jobs = q.order_by_desc(apalis_job::Column::RunAt).all(db).await?;

    // Recursively build children for each node, converting Model → JsonValue.
    let mut result = Vec::with_capacity(jobs.len());
    for job in jobs {
        let children = Box::pin(list_children(db, &job.id, task_type, status)).await?;
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

/// Return child jobs (one level) for a given parent, with optional type/status filters.
pub async fn list_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<apalis_job::Model>, DbErr> {
    // Step 1: Get child_job_id values for this parent
    let child_ids: Vec<String> = TaskDependency::find()
        .filter(task_dependency::Column::ParentJobId.eq(parent_id))
        .all(db)
        .await?
        .into_iter()
        .map(|d| d.child_job_id)
        .collect();

    if child_ids.is_empty() {
        return Ok(vec![]);
    }

    // Step 2: Fetch the actual job records
    let mut q = ApalisJob::find().filter(apalis_job::Column::Id.is_in(child_ids));

    if let Some(tt) = task_type {
        q = q.filter(apalis_job::Column::JobType.eq(tt));
    }
    if let Some(st) = status {
        q = q.filter(apalis_job::Column::Status.eq(st));
    }
    q.order_by_desc(apalis_job::Column::RunAt).all(db).await
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
        .add(apalis_job::Column::Status.eq(apalis_job::STATUS_PENDING));

    if let Some(tt) = job_type {
        cond = cond.add(apalis_job::Column::JobType.eq(tt));
    }

    let to_delete = ApalisJob::find().filter(cond).all(db).await?;
    let deleted_ids: Vec<String> = to_delete.iter().map(|j| j.id.clone()).collect();

    if deleted_ids.is_empty() {
        return Ok((vec![], 0));
    }

    // Delete task_dependency rows that reference the deleted children
    let dep_deleted = TaskDependency::delete_many()
        .filter(task_dependency::Column::ChildJobId.is_in(&deleted_ids))
        .exec(db)
        .await?;

    // Delete the jobs themselves
    ApalisJob::delete_many()
        .filter(apalis_job::Column::Id.is_in(&deleted_ids))
        .exec(db)
        .await?;

    Ok((deleted_ids, dep_deleted.rows_affected))
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
