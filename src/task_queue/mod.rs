pub mod handlers;
pub mod jobs;
pub mod runner;
pub mod tasks;

use sea_orm::*;
use chrono::Utc;
use crate::db::entities::task::{self, Entity as TaskEntity};

/// Submit a new task to the queue
pub async fn submit_task(
    db: &DatabaseConnection,
    task_type: &str,
    payload: serde_json::Value,
    priority: i32,
) -> Result<task::Model, DbErr> {
    // Extract image_id / image_path from payload for top-level indexing
    let image_id = payload
        .get("image_id")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let image_path = payload
        .get("image_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let model = task::ActiveModel {
        id: Set(uuid::Uuid::new_v4().to_string()),
        task_type: Set(task_type.to_string()),
        payload: Set(payload),
        image_id: Set(image_id),
        image_path: Set(image_path),
        status: Set("pending".to_string()),
        priority: Set(priority),
        retry_count: Set(0),
        max_retries: Set(3),
        created_at: Set(Utc::now().naive_utc()),
        ..Default::default()
    };
    model.insert(db).await
}

/// Claim next pending task atomically using a single UPDATE with subquery.
/// This avoids the race condition of a separate SELECT + UPDATE.
pub async fn claim_next_task(
    db: &DatabaseConnection,
    task_type: &str,
) -> Result<Option<task::Model>, DbErr> {
    let now = Utc::now().naive_utc();

    let stmt = Statement::from_sql_and_values(
        db.get_database_backend(),
        r#"UPDATE background_tasks
           SET status = 'running', started_at = $1
           WHERE id = (
               SELECT id FROM background_tasks
               WHERE status = 'pending' AND task_type = $2
               ORDER BY priority DESC, created_at ASC
               LIMIT 1
           )
           RETURNING id, task_type, payload, image_id, image_path,
                     status, priority, retry_count, max_retries,
                     created_at, started_at, finished_at, last_error"#,
        vec![now.into(), task_type.into()],
    );

    let Some(row) = db.query_one(stmt).await? else {
        return Ok(None);
    };

    // Map QueryResult columns to task::Model by index (matching RETURNING order)
    Ok(Some(task::Model {
        id: row.try_get_by_index(0)?,
        task_type: row.try_get_by_index(1)?,
        payload: row.try_get_by_index(2)?,
        image_id: row.try_get_by_index(3)?,
        image_path: row.try_get_by_index(4)?,
        status: row.try_get_by_index(5)?,
        priority: row.try_get_by_index(6)?,
        retry_count: row.try_get_by_index(7)?,
        max_retries: row.try_get_by_index(8)?,
        created_at: row.try_get_by_index(9)?,
        started_at: row.try_get_by_index(10)?,
        finished_at: row.try_get_by_index(11)?,
        last_error: row.try_get_by_index(12)?,
    }))
}

/// Mark task completed
pub async fn complete_task(
    db: &DatabaseConnection,
    task_id: &str,
) -> Result<(), DbErr> {
    if let Some(t) = TaskEntity::find_by_id(task_id).one(db).await? {
        let mut active: task::ActiveModel = t.into();
        active.status = Set("completed".to_string());
        active.finished_at = Set(Some(Utc::now().naive_utc()));
        active.update(db).await?;
    }
    Ok(())
}

/// Mark task failed (with retry)
pub async fn fail_task(
    db: &DatabaseConnection,
    task_id: &str,
    error: &str,
) -> Result<(), DbErr> {
    if let Some(t) = TaskEntity::find_by_id(task_id).one(db).await? {
        let mut active: task::ActiveModel = t.clone().into();
        let new_retry = t.retry_count + 1;

        if new_retry < t.max_retries {
            active.status = Set("pending".to_string());
            active.retry_count = Set(new_retry);
            active.last_error = Set(Some(error.to_string()));
            active.started_at = Set(None);
        } else {
            active.status = Set("failed".to_string());
            active.finished_at = Set(Some(Utc::now().naive_utc()));
            active.last_error = Set(Some(error.to_string()));
        }
        active.update(db).await?;
    }
    Ok(())
}
