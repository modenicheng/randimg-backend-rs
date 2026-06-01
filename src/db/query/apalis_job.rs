use crate::db::entities::apalis_job::{self, Entity as ApalisJob};
use sea_orm::*;

/// List jobs with optional type/status filters, ordered by run_at descending.
pub async fn list(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    status: Option<Vec<&str>>,
    limit: u64,
    offset: u64,
) -> Result<Vec<apalis_job::Model>, DbErr> {
    let mut query = ApalisJob::find().order_by_desc(apalis_job::Column::RunAt);
    if let Some(tt) = task_type {
        query = query.filter(apalis_job::Column::JobType.eq(tt));
    }
    if let Some(ref statuses) = status {
        if !statuses.is_empty() {
            query = query.filter(apalis_job::Column::Status.is_in(statuses.iter().copied()));
        }
    }
    query.limit(limit).offset(offset).all(db).await
}

/// Count jobs with optional type/status filters.
pub async fn count(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    status: Option<Vec<&str>>,
) -> Result<u64, DbErr> {
    let mut query = ApalisJob::find();
    if let Some(tt) = task_type {
        query = query.filter(apalis_job::Column::JobType.eq(tt));
    }
    if let Some(ref statuses) = status {
        if !statuses.is_empty() {
            query = query.filter(apalis_job::Column::Status.is_in(statuses.iter().copied()));
        }
    }
    query.count(db).await
}

/// Find a single job by ID.
pub async fn find_by_id(
    db: &DatabaseConnection,
    id: &str,
) -> Result<Option<apalis_job::Model>, DbErr> {
    ApalisJob::find_by_id(id.to_string()).one(db).await
}

/// Delete a job by ID. Returns `true` if a row was deleted.
pub async fn delete_by_id(
    db: &DatabaseConnection,
    id: &str,
) -> Result<bool, DbErr> {
    let result = ApalisJob::delete_many()
        .filter(apalis_job::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Delete all pending jobs, optionally filtered by job type.
/// Returns the number of rows deleted.
pub async fn delete_pending(
    db: &DatabaseConnection,
    task_type: Option<&str>,
) -> Result<u64, DbErr> {
    delete_by_statuses(
        db,
        &[apalis_job::STATUS_PENDING, apalis_job::STATUS_QUEUED],
        task_type,
    )
    .await
}

/// Bulk-delete jobs by statuses.
///
/// Deletes from both `Jobs` and `task_dependencies` (as child) to avoid
/// orphaned dependency rows. Returns the number of jobs deleted.
pub async fn delete_by_statuses(
    db: &DatabaseConnection,
    statuses: &[&str],
    task_type: Option<&str>,
) -> Result<u64, DbErr> {
    use crate::db::entities::task_dependency::{Entity as TaskDependency, Column as DepCol};

    if statuses.is_empty() {
        return Ok(0);
    }

    // Step 1: Find job IDs matching the filters
    let mut q = ApalisJob::find()
        .select_only()
        .column(apalis_job::Column::Id)
        .filter(apalis_job::Column::Status.is_in(statuses.iter().copied()));
    if let Some(tt) = task_type {
        q = q.filter(apalis_job::Column::JobType.eq(tt));
    }
    let ids: Vec<String> = q.into_tuple().all(db).await?;

    if ids.is_empty() {
        return Ok(0);
    }

    // Step 2 & 3: Delete dependencies and jobs atomically
    let ids_clone = ids.clone();
    let result = db.transaction::<_, u64, DbErr>(|txn| {
        Box::pin(async move {
            // Delete task_dependency rows where these jobs are children
            TaskDependency::delete_many()
                .filter(DepCol::ChildJobId.is_in(&ids_clone))
                .exec(txn)
                .await?;

            // Delete task_dependency rows where these jobs are parents
            TaskDependency::delete_many()
                .filter(DepCol::ParentJobId.is_in(&ids_clone))
                .exec(txn)
                .await?;

            // Delete the jobs themselves
            let result = ApalisJob::delete_many()
                .filter(apalis_job::Column::Id.is_in(&ids_clone))
                .exec(txn)
                .await?;

            Ok(result.rows_affected)
        })
    })
    .await
    .map_err(|e| match e {
        TransactionError::Connection(e) => e,
        TransactionError::Transaction(e) => e,
    })?;

    Ok(result)
}
