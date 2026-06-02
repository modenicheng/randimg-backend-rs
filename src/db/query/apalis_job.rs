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

/// Delete a job by ID, along with any `task_dependencies` rows that reference it
/// (both as parent and as child). Returns `true` if the job row was deleted.
pub async fn delete_by_id(
    db: &DatabaseConnection,
    id: &str,
) -> Result<bool, DbErr> {
    use crate::db::entities::task_dependency::{Entity as TaskDependency, Column as DepCol};

    let id_owned = id.to_string();
    let result = db.transaction::<_, bool, DbErr>(|txn| {
        Box::pin(async move {
            // Delete task_dependency rows where this job is a child
            TaskDependency::delete_many()
                .filter(DepCol::ChildJobId.eq(&id_owned))
                .exec(txn)
                .await?;

            // Delete task_dependency rows where this job is a parent
            TaskDependency::delete_many()
                .filter(DepCol::ParentJobId.eq(&id_owned))
                .exec(txn)
                .await?;

            // Delete the job itself
            let result = ApalisJob::delete_many()
                .filter(apalis_job::Column::Id.eq(&id_owned))
                .exec(txn)
                .await?;

            Ok(result.rows_affected > 0)
        })
    })
    .await
    .map_err(|e| match e {
        TransactionError::Connection(e) => e,
        TransactionError::Transaction(e) => e,
    })?;

    Ok(result)
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

/// Find IDs of crawl tasks whose payload has the given crawl_type.
#[cfg(feature = "sqlite")]
pub async fn find_crawl_ids_by_type(
    db: &DatabaseConnection,
    crawl_type: i32,
) -> Result<Vec<String>, DbErr> {
    let sql = "SELECT id FROM Jobs WHERE job_type = 'randimg_backend_rs::task_queue::jobs::CrawlJob' AND json_extract(job, '$.crawl_type') = ?";
    let stmt = sea_orm::Statement::from_sql_and_values(
        db.get_database_backend(),
        sql,
        [crawl_type.into()],
    );
    let rows = db.query_all(stmt).await?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in &rows {
        ids.push(row.try_get_by_index::<String>(0)?);
    }
    Ok(ids)
}

/// Find IDs of crawl tasks whose payload has the given crawl_type.
#[cfg(feature = "postgres")]
pub async fn find_crawl_ids_by_type(
    db: &DatabaseConnection,
    crawl_type: i32,
) -> Result<Vec<String>, DbErr> {
    let sql = "SELECT id FROM apalis.jobs WHERE job_type = 'randimg_backend_rs::task_queue::jobs::CrawlJob' AND (job::json->>'crawl_type')::int = $1";
    let stmt = sea_orm::Statement::from_sql_and_values(
        db.get_database_backend(),
        sql,
        [crawl_type.into()],
    );
    let rows = db.query_all(stmt).await?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in &rows {
        ids.push(row.try_get_by_index::<String>(0)?);
    }
    Ok(ids)
}

/// Bulk-delete jobs by statuses and a set of IDs.
///
/// Deletes only jobs that match both the status list AND the ID list.
/// Also cleans up task_dependencies rows. Returns the number of jobs deleted.
pub async fn delete_by_statuses_and_ids(
    db: &DatabaseConnection,
    statuses: &[&str],
    ids: &[String],
) -> Result<u64, DbErr> {
    use crate::db::entities::task_dependency::{Entity as TaskDependency, Column as DepCol};

    if statuses.is_empty() || ids.is_empty() {
        return Ok(0);
    }

    // Find matching jobs
    let to_delete: Vec<String> = ApalisJob::find()
        .select_only()
        .column(apalis_job::Column::Id)
        .filter(apalis_job::Column::Status.is_in(statuses.iter().copied()))
        .filter(apalis_job::Column::Id.is_in(ids.iter().map(|s| s.as_str())))
        .into_tuple()
        .all(db)
        .await?;

    if to_delete.is_empty() {
        return Ok(0);
    }

    let ids_clone = to_delete.clone();
    let result = db.transaction::<_, u64, DbErr>(|txn| {
        Box::pin(async move {
            TaskDependency::delete_many()
                .filter(DepCol::ChildJobId.is_in(&ids_clone))
                .exec(txn)
                .await?;

            TaskDependency::delete_many()
                .filter(DepCol::ParentJobId.is_in(&ids_clone))
                .exec(txn)
                .await?;

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
