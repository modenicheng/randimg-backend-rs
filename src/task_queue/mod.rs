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

/// Claim next pending task (atomic via UPDATE ... WHERE status='pending')
pub async fn claim_next_task(
    db: &DatabaseConnection,
    task_type: &str,
) -> Result<Option<task::Model>, DbErr> {
    // Find the best candidate
    let pending = TaskEntity::find()
        .filter(task::Column::Status.eq("pending"))
        .filter(task::Column::TaskType.eq(task_type))
        .order_by_desc(task::Column::Priority)
        .order_by_asc(task::Column::CreatedAt)
        .one(db)
        .await?;

    let Some(t) = pending else {
        return Ok(None);
    };

    // Atomic update: only succeed if still pending (prevents race condition)
    let now = Utc::now().naive_utc();
    let update_result = task::ActiveModel {
        id: Set(t.id.clone()),
        status: Set("running".to_string()),
        started_at: Set(Some(now)),
        ..Default::default()
    }
    .update(db)
    .await;

    match update_result {
        Ok(updated) => {
            // Verify we actually claimed it (status should be running now)
            if updated.status == "running" {
                Ok(Some(updated))
            } else {
                // Another worker claimed it first
                Ok(None)
            }
        }
        Err(_) => Ok(None),
    }
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
