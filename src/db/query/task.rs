use sea_orm::*;
use crate::db::entities::task::{self, Entity as TaskEntity};

/// Find a single task by ID.
pub async fn find_by_id(
    db: &DatabaseConnection,
    task_id: &str,
) -> Result<Option<task::Model>, DbErr> {
    TaskEntity::find_by_id(task_id).one(db).await
}

/// List all tasks, optionally filtered by task_type and/or status.
pub async fn find_filtered(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    status: Option<&str>,
    limit: u64,
) -> Result<Vec<task::Model>, DbErr> {
    let mut query = TaskEntity::find()
        .order_by_desc(task::Column::CreatedAt);

    if let Some(tt) = task_type {
        query = query.filter(task::Column::TaskType.eq(tt));
    }
    if let Some(s) = status {
        query = query.filter(task::Column::Status.eq(s));
    }

    query.limit(limit).all(db).await
}

/// Retry a failed task: reset status to pending, clear error, bump retry_count.
pub async fn retry_task(
    db: &DatabaseConnection,
    task_id: &str,
) -> Result<Option<task::Model>, DbErr> {
    if let Some(t) = TaskEntity::find_by_id(task_id).one(db).await? {
        if t.status != "failed" {
            return Err(DbErr::Custom(
                "Only failed tasks can be retried".into(),
            ));
        }
        let mut active: task::ActiveModel = t.into();
        active.status = Set("pending".to_string());
        active.last_error = Set(None);
        active.started_at = Set(None);
        active.finished_at = Set(None);
        let updated = active.update(db).await?;
        Ok(Some(updated))
    } else {
        Ok(None)
    }
}
