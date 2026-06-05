/// Dead letter query functions
///
/// Provides CRUD operations for the dead_letter table.
/// Tasks that exceed max retries are moved here with full failure metadata.
use crate::db::entities::dead_letter::{self, Entity as DeadLetter};
use chrono::Utc;
use sea_orm::*;
use serde_json::json;

/// Insert a dead letter entry for a permanently failed task.
pub async fn insert_dead_letter(
    db: &DatabaseConnection,
    task_id: &str,
    task_type: &str,
    params: Option<&str>,
    error_message: &str,
    retry_count: i32,
    failure_history: Option<serde_json::Value>,
) -> Result<dead_letter::Model, DbErr> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();

    let model = dead_letter::ActiveModel {
        id: Set(id),
        task_id: Set(task_id.to_string()),
        task_type: Set(task_type.to_string()),
        params: Set(params.map(|s| s.to_string())),
        error_message: Set(error_message.to_string()),
        retry_count: Set(retry_count),
        failure_history: Set(Some(
            failure_history.unwrap_or_else(|| json!([])),
        )),
        created_at: Set(now.into()),
    };
    model.insert(db).await
}

/// List dead letter entries with optional task_type filter and pagination.
pub async fn list_dead_letters(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    limit: u64,
    offset: u64,
) -> Result<Vec<dead_letter::Model>, DbErr> {
    let mut query = DeadLetter::find().order_by_desc(dead_letter::Column::CreatedAt);
    if let Some(tt) = task_type {
        query = query.filter(dead_letter::Column::TaskType.eq(tt));
    }
    query.limit(limit).offset(offset).all(db).await
}

/// Get a single dead letter entry by ID.
pub async fn get_dead_letter(
    db: &DatabaseConnection,
    id: &str,
) -> Result<Option<dead_letter::Model>, DbErr> {
    DeadLetter::find_by_id(id.to_string()).one(db).await
}

/// Requeue a dead letter: creates a new task and deletes the dead letter entry.
///
/// The new task starts in `pending` status with `retry_count` reset to 0.
/// Returns the newly created task.
pub async fn requeue_dead_letter(
    db: &DatabaseConnection,
    id: &str,
) -> Result<crate::db::entities::task::Model, DbErr> {
    let dl = DeadLetter::find_by_id(id.to_string())
        .one(db)
        .await?
        .ok_or(DbErr::RecordNotFound(format!(
            "Dead letter {id} not found"
        )))?;

    // Create a new task from the dead letter data
    let task_type: crate::db::entities::task_enum::TaskType = dl.task_type.parse()
        .map_err(|e: String| DbErr::Custom(format!("Invalid task type '{}': {}", dl.task_type, e)))?;
    let new_task = super::task::create(
        db,
        task_type,
        None,  // parent_id: original parent may no longer exist
        None,  // root_id
        None,  // crawler_id
        None,  // image_id
        dl.params.as_deref(),
    )
    .await?;

    // Delete the dead letter entry
    DeadLetter::delete_many()
        .filter(dead_letter::Column::Id.eq(id))
        .exec(db)
        .await?;

    Ok(new_task)
}

/// Delete a dead letter entry by ID.
pub async fn delete_dead_letter(db: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
    let result = DeadLetter::delete_many()
        .filter(dead_letter::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

pub async fn delete_older_than(
    db: &DatabaseConnection,
    older_than_hours: i64,
) -> Result<u64, DbErr> {
    if older_than_hours <= 0 {
        return Ok(0);
    }

    let cutoff = Utc::now() - chrono::Duration::hours(older_than_hours);

    let result = DeadLetter::delete_many()
        .filter(dead_letter::Column::CreatedAt.lt(cutoff))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}
