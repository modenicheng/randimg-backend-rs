use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use crate::AppState;
use super::{claim_next_task, complete_task, fail_task};
use super::tasks;

/// Start background task processing loops.
/// Returns JoinHandles so the caller can abort them on shutdown.
pub fn start_runner(state: Arc<AppState>) -> Vec<JoinHandle<()>> {
    let task_types = vec![
        "color_extract",
        "discover",
        "download",
        "upload",
        "crawl",
        "accessibility_check",
    ];

    task_types
        .into_iter()
        .map(|task_type| {
            let state = state.clone();
            let name = task_type.to_owned();
            tokio::spawn(async move {
                tracing::info!(task_type = %name, "Task runner started");
                loop {
                    match claim_next_task(&state.db, task_type).await {
                    Ok(Some(task)) => {
                        tracing::info!("Processing task {}: {}", task.id, task.task_type);
                        let result = match task.task_type.as_str() {
                            "color_extract" => tasks::color_extract::run(&state, &task).await,
                            "discover" => tasks::discover::run(&state, &task).await,
                            "download" => tasks::download::run(&state, &task).await,
                            "upload" => tasks::upload::run(&state, &task).await,
                            "crawl" => tasks::crawl::run(&state, &task).await,
                            "accessibility_check" => tasks::accessibility_check::run(&state, &task).await,
                            _ => Ok(()),
                        };

                        match result {
                            Ok(()) => {
                                if let Err(e) = complete_task(&state.db, &task.id).await {
                                    tracing::error!("Failed to complete task {}: {}", task.id, e);
                                }
                            }
                            Err(e) => {
                                tracing::error!("Task {} failed: {}", task.id, e);
                                if let Err(db_err) = fail_task(&state.db, &task.id, &e).await {
                                    tracing::error!("Failed to mark task failed: {}", db_err);
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        sleep(Duration::from_secs(5)).await;
                    }
                    Err(e) => {
                        tracing::error!("Error claiming task: {}", e);
                        sleep(Duration::from_secs(10)).await;
                    }
                }
            }
        })
        })
        .collect()
}
