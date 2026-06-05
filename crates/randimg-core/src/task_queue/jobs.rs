use std::sync::Arc;

use fang::asynk::async_queue::AsyncQueueable;
use fang::asynk::async_runnable::AsyncRunnable;
use fang::async_trait;
use fang::typetag;
use fang::FangError;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

use crate::WorkerState;
use crate::db::entities::task_enum::TaskStatus;
use crate::db::query;
use super::handlers;
use serde_json::json;

// ── AsyncRunnable macro ────────────────────────────────────────

/// Check if a task has exceeded max retries and move it to the dead letter queue.
///
/// Called after a task is marked as `failed`. If `retry_count >= max_retries`,
/// the task status is updated to `dead` and a dead letter entry is created.
async fn maybe_move_to_dead_letter(
    state: &Arc<WorkerState>,
    task_id: &str,
    max_retries: i32,
    error_message: &str,
    task_type: &str,
) {
    let task = match query::task::find_by_id(&state.db, task_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            tracing::error!(task_id, "Task not found for DLQ check");
            return;
        }
        Err(e) => {
            tracing::error!(task_id, error = %e, "Failed to fetch task for DLQ check");
            return;
        }
    };

    if task.retry_count < max_retries {
        return;
    }

    tracing::warn!(
        task_id,
        task_type,
        retry_count = task.retry_count,
        max_retries,
        "Task exceeded max retries, moving to dead letter queue"
    );

    if let Err(e) = query::task::update_status(&state.db, task_id, TaskStatus::Dead).await {
        tracing::error!(task_id, error = %e, "Failed to update task status to dead");
    }

    let mut history: Vec<serde_json::Value> = Vec::new();
    if let Some(ref prev_err) = task.error_message {
        if !prev_err.is_empty() {
            history.push(json!({
                "attempt": task.retry_count,
                "error": prev_err,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }));
        }
    }
    history.push(json!({
        "attempt": task.retry_count + 1,
        "error": error_message,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }));

    if let Err(e) = query::dead_letter::insert_dead_letter(
        &state.db,
        task_id,
        task_type,
        task.params.as_deref(),
        error_message,
        task.retry_count,
        Some(json!(history)),
    )
    .await
    {
        tracing::error!(task_id, error = %e, "Failed to insert dead letter entry");
    }
}

/// Generate an `AsyncRunnable` implementation with common status-tracking
/// boilerplate.  Usage:
///
/// ```ignore
/// impl_async_runnable!(CrawlJob, handle_crawl, "crawl");
/// ```
macro_rules! impl_async_runnable {
    ($job:ty, $handler:ident, $task_type:expr) => {
        #[typetag::serde]
        #[async_trait]
        impl AsyncRunnable for $job {
            async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
                let state = worker_state();

                // Update status to running
                if let Some(ref task_id) = self.task_id {
                    if let Err(e) = query::task::update_status(&state.db, task_id, TaskStatus::Running).await {
                        tracing::error!(task_id, error = %e, "Failed to update task status to running");
                    }
                }

                let timeout_secs = state.config.task_default_timeout_secs;
                tracing::info!(task_type = $task_type, timeout_secs, "Job started");
                let timed_result = tokio::time::timeout(
                    std::time::Duration::from_secs(timeout_secs),
                    handlers::$handler(self.clone(), state),
                ).await;

                match timed_result {
                    Ok(result) => {
                        // Handler completed within timeout (may have succeeded or failed)
                        if let Some(ref task_id) = self.task_id {
                            match &result {
                                Ok(()) => {
                                    if let Err(e) = query::task::update_status(&state.db, task_id, TaskStatus::Done).await {
                                        tracing::error!(task_id, error = %e, "Failed to update task status to done");
                                    }
                                }
                                Err(e) => {
                                    if let Err(update_err) = query::task::update_status(&state.db, task_id, TaskStatus::Failed).await {
                                        tracing::error!(task_id, error = %update_err, "Failed to update task status to failed");
                                    }
                                    if let Err(update_err) = query::task::update_error(&state.db, task_id, &e.to_string()).await {
                                        tracing::error!(task_id, error = %update_err, "Failed to update task error message");
                                    }
                                    maybe_move_to_dead_letter(state, task_id, self.max_retries, &e.to_string(), $task_type).await;
                                }
                            }
                        }

                        match &result {
                            Ok(()) => tracing::info!(task_type = $task_type, "Job completed"),
                            Err(e) => tracing::error!(task_type = $task_type, error = %e, "Job failed"),
                        }

                        // Record timestamp for watchdog
                        state.last_activity.insert($task_type.to_string(), std::time::Instant::now());

                        result.map_err(|e| FangError { description: e })
                    }
                    Err(_elapsed) => {
                        let timeout_msg = format!("Task timed out after {}s", timeout_secs);
                        if let Some(ref task_id) = self.task_id {
                            if let Err(e) = query::task::update_status(&state.db, task_id, TaskStatus::Failed).await {
                                tracing::error!(task_id, error = %e, "Failed to update task status to failed");
                            }
                            if let Err(e) = query::task::update_error(&state.db, task_id, &timeout_msg).await {
                                tracing::error!(task_id, error = %e, "Failed to update task error message");
                            }
                            maybe_move_to_dead_letter(state, task_id, self.max_retries, &timeout_msg, $task_type).await;
                        }
                        tracing::error!(task_type = $task_type, timeout_secs, "Task timed out");

                        // Record timestamp for watchdog even on timeout
                        state.last_activity.insert($task_type.to_string(), std::time::Instant::now());

                        // Return Ok to prevent Fang from retrying (already marked as failed)
                        Ok(())
                    }
                }
            }

            fn task_type(&self) -> String {
                $task_type.to_string()
            }

            fn max_retries(&self) -> i32 {
                self.max_retries
            }

            fn backoff(&self, attempt: u32) -> u32 {
                u32::pow(self.backoff_base, attempt)
            }
        }
    };
}

fn default_max_retries() -> i32 { 3 }
fn default_backoff_base() -> u32 { 2 }

// ── Global WorkerState accessor ────────────────────────────────

static WORKER_STATE: OnceCell<Arc<WorkerState>> = OnceCell::const_new();

/// Initialize the global WorkerState. Must be called once at startup.
pub async fn init_worker_state(state: Arc<WorkerState>) {
    if WORKER_STATE.set(state).is_err() {
        panic!("WorkerState already initialized");
    }
}

/// Get a reference to the global WorkerState.
///
/// # Panics
///
/// Panics if `init_worker_state()` has not been called yet.
pub fn worker_state() -> &'static Arc<WorkerState> {
    WORKER_STATE.get().expect("WorkerState not initialized")
}

/// Crawl Pixiv illustrations (by user, ranking, or bookmarks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlJob {
    pub crawler_id: i32,
    pub crawl_type: i32,
    pub target_user_id: Option<String>,
    pub target_start_date: Option<String>,
    pub target_end_date: Option<String>,
    pub target_search_prompt: Option<String>,
    /// Ranking mode: "day", "week", "month", "original", "rookie", "daily_r18", "weekly_r18" (default: "day").
    #[serde(default)]
    pub ranking_mode: Option<String>,
    /// User illust type: "illust", "manga" (default: "illust").
    #[serde(default)]
    pub illust_type: Option<String>,
    /// Filter by illust type: list of types to include (e.g., ["illust", "manga"]).
    /// If None or empty, all types are included. Default: None (no filter).
    #[serde(default)]
    pub illust_type_filter: Option<Vec<String>>,
    /// Exclude R18 content (x_restrict > 0). Default: false.
    #[serde(default)]
    pub exclude_r18: Option<bool>,
    /// Exclude AI-generated content (illust_ai_type > 0). Default: false.
    #[serde(default)]
    pub exclude_ai: Option<bool>,
    /// Maximum total pages to crawl (0 or None = unlimited).
    #[serde(default)]
    pub max_pages: Option<u32>,
    /// Max discover hops to run after crawl (override global default).
    #[serde(default)]
    pub discover_hops: Option<u32>,
    /// Max seed limit for discover (override global default).
    #[serde(default)]
    pub discover_seed_limit: Option<u64>,
    /// Seed selection method for discover: "popularity", "views", "bookmarks", "random".
    #[serde(default)]
    pub discover_seed_method: Option<String>,
    /// Disable discover entirely for this crawl job. Default: false.
    #[serde(default)]
    pub disable_discover: Option<bool>,
    /// Specific Pixiv credential IDs to use. If set, only these credentials
    /// will be considered for authentication. If empty/None, falls back to
    /// random active credential selection.
    #[serde(default)]
    pub credential_ids: Option<Vec<i32>>,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
    /// Maximum retry attempts (0 = no retry). Populated from AppConfig.
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    /// Backoff base in seconds for exponential retry. Populated from AppConfig.
    #[serde(default = "default_backoff_base")]
    pub backoff_base: u32,
}

/// Download a single image from Pixiv to local disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadJob {
    pub image_id: i32,
    pub source_image_url: String,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// The ID of the root crawl job that originated this pipeline.
    /// Downstream tasks (color_extract, upload, accessibility_check)
    /// use this as their `parent_job_id` so the full pipeline is
    /// represented as direct children of the crawl task.
    #[serde(default)]
    pub root_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
    /// Maximum retry attempts (0 = no retry). Populated from AppConfig.
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    /// Backoff base in seconds for exponential retry. Populated from AppConfig.
    #[serde(default = "default_backoff_base")]
    pub backoff_base: u32,
}

/// Extract color palette from a downloaded image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorExtractJob {
    pub image_id: i32,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
    /// Maximum retry attempts (0 = no retry). Populated from AppConfig.
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    /// Backoff base in seconds for exponential retry. Populated from AppConfig.
    #[serde(default = "default_backoff_base")]
    pub backoff_base: u32,
}

/// Upload a downloaded image to DogeCloud OSS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadJob {
    pub image_id: i32,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
    /// Maximum retry attempts (0 = no retry). Populated from AppConfig.
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    /// Backoff base in seconds for exponential retry. Populated from AppConfig.
    #[serde(default = "default_backoff_base")]
    pub backoff_base: u32,
}

/// Check image accessibility (solid-color detection stub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityCheckJob {
    pub image_id: i32,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
    /// Maximum retry attempts (0 = no retry). Populated from AppConfig.
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    /// Backoff base in seconds for exponential retry. Populated from AppConfig.
    #[serde(default = "default_backoff_base")]
    pub backoff_base: u32,
}

/// Discover related illustrations via Pixiv related-illust API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverJob {
    pub hop: u32,
    pub max_hops: Option<u32>,
    pub seed_limit: Option<u64>,
    pub seed_method: Option<String>,
    /// Specific Pixiv credential IDs to use (inherited from parent CrawlJob).
    #[serde(default)]
    pub credential_ids: Option<Vec<i32>>,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
    /// The ID of the root crawl job that originated this discovery chain.
    /// Used to maintain flat tree hierarchy for API pagination.
    #[serde(default)]
    pub root_job_id: Option<String>,
    /// Maximum retry attempts (0 = no retry). Populated from AppConfig.
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    /// Backoff base in seconds for exponential retry. Populated from AppConfig.
    #[serde(default = "default_backoff_base")]
    pub backoff_base: u32,
}

/// Refresh a Pixiv credential's OAuth token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshPixivTokenJob {
    pub credential_id: i32,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
    /// Maximum retry attempts (0 = no retry). Populated from AppConfig.
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    /// Backoff base in seconds for exponential retry. Populated from AppConfig.
    #[serde(default = "default_backoff_base")]
    pub backoff_base: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupJob {
    #[serde(default)]
    pub parent_job_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    #[serde(default = "default_backoff_base")]
    pub backoff_base: u32,
}

// ── AsyncRunnable implementations ──────────────────────────────

impl_async_runnable!(CrawlJob, handle_crawl, "crawl");
impl_async_runnable!(DownloadJob, handle_download, "download");
impl_async_runnable!(ColorExtractJob, handle_color_extract, "color_extract");
impl_async_runnable!(UploadJob, handle_upload, "upload");
impl_async_runnable!(AccessibilityCheckJob, handle_accessibility_check, "accessibility_check");
impl_async_runnable!(DiscoverJob, handle_discover, "discover");
impl_async_runnable!(RefreshPixivTokenJob, handle_refresh_pixiv_token, "refresh_pixiv_token");
impl_async_runnable!(CleanupJob, handle_cleanup, "cleanup");
