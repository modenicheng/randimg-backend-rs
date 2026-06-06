use std::sync::Arc;

use crate::WorkerState;
use crate::db::query;

use super::super::jobs::*;

pub async fn handle_cleanup(_job: CleanupJob, state: &Arc<WorkerState>) -> Result<(), String> {
    let task_ttl = state.config.task_cleanup_ttl_hours as i64;
    let dl_ttl = state.config.dead_letter_ttl_hours as i64;

    let deleted_tasks = query::task::delete_by_statuses_and_older_than(
        &state.db,
        &[
            crate::db::entities::task_enum::TaskStatus::Done,
            crate::db::entities::task_enum::TaskStatus::Failed,
            crate::db::entities::task_enum::TaskStatus::Dead,
        ],
        task_ttl,
    )
    .await
    .map_err(|e| format!("Failed to cleanup old tasks: {}", e))?;

    let deleted_dl = query::dead_letter::delete_older_than(&state.db, dl_ttl)
        .await
        .map_err(|e| format!("Failed to cleanup old dead letters: {}", e))?;

    tracing::info!(
        deleted_tasks,
        deleted_dl,
        task_ttl_hours = task_ttl,
        dead_letter_ttl_hours = dl_ttl,
        "Cleanup completed"
    );

    Ok(())
}
