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
    let mut delete = ApalisJob::delete_many()
        .filter(apalis_job::Column::Status.eq(apalis_job::STATUS_PENDING));
    if let Some(tt) = task_type {
        delete = delete.filter(apalis_job::Column::JobType.eq(tt));
    }
    let result = delete.exec(db).await?;
    Ok(result.rows_affected)
}
