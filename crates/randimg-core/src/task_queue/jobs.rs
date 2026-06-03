use std::sync::Arc;

use fang::asynk::async_queue::AsyncQueueable;
use fang::asynk::async_runnable::AsyncRunnable;
use fang::async_trait;
use fang::typetag;
use fang::FangError;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

use crate::WorkerState;
use super::handlers;

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
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
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
}

/// Extract color palette from a downloaded image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorExtractJob {
    pub image_id: i32,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
}

/// Upload a downloaded image to DogeCloud OSS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadJob {
    pub image_id: i32,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
}

/// Check image accessibility (solid-color detection stub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityCheckJob {
    pub image_id: i32,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
}

/// Discover related illustrations via Pixiv related-illust API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverJob {
    pub hop: u32,
    pub max_hops: Option<u32>,
    pub seed_limit: Option<u64>,
    pub seed_method: Option<String>,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
}

/// Refresh a Pixiv credential's OAuth token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshPixivTokenJob {
    pub credential_id: i32,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
}

// ── AsyncRunnable implementations ──────────────────────────────

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for CrawlJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        tracing::info!(crawler_id = self.crawler_id, crawl_type = self.crawl_type, "CrawlJob started");
        let state = worker_state();
        let result = handlers::handle_crawl(self.clone(), state).await;
        match &result {
            Ok(()) => tracing::info!(crawler_id = self.crawler_id, "CrawlJob completed"),
            Err(e) => tracing::error!(crawler_id = self.crawler_id, error = %e, "CrawlJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    fn task_type(&self) -> String {
        "crawl".to_string()
    }

    fn max_retries(&self) -> i32 {
        3
    }

    fn backoff(&self, attempt: u32) -> u32 {
        u32::pow(2, attempt)
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for DownloadJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        tracing::info!(image_id = self.image_id, path = %self.image_path, "DownloadJob started");
        let state = worker_state();
        let result = handlers::handle_download(self.clone(), state).await;
        match &result {
            Ok(()) => tracing::info!(image_id = self.image_id, "DownloadJob completed"),
            Err(e) => tracing::error!(image_id = self.image_id, error = %e, "DownloadJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    fn task_type(&self) -> String {
        "download".to_string()
    }

    fn max_retries(&self) -> i32 {
        3
    }

    fn backoff(&self, attempt: u32) -> u32 {
        u32::pow(2, attempt)
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for ColorExtractJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        tracing::info!(image_id = self.image_id, path = %self.image_path, "ColorExtractJob started");
        let state = worker_state();
        let result = handlers::handle_color_extract(self.clone(), state).await;
        match &result {
            Ok(()) => tracing::info!(image_id = self.image_id, "ColorExtractJob completed"),
            Err(e) => tracing::error!(image_id = self.image_id, error = %e, "ColorExtractJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    fn task_type(&self) -> String {
        "color_extract".to_string()
    }

    fn max_retries(&self) -> i32 {
        3
    }

    fn backoff(&self, attempt: u32) -> u32 {
        u32::pow(2, attempt)
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for UploadJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        tracing::info!(image_id = self.image_id, path = %self.image_path, "UploadJob started");
        let state = worker_state();
        let result = handlers::handle_upload(self.clone(), state).await;
        match &result {
            Ok(()) => tracing::info!(image_id = self.image_id, "UploadJob completed"),
            Err(e) => tracing::error!(image_id = self.image_id, error = %e, "UploadJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    fn task_type(&self) -> String {
        "upload".to_string()
    }

    fn max_retries(&self) -> i32 {
        3
    }

    fn backoff(&self, attempt: u32) -> u32 {
        u32::pow(2, attempt)
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for AccessibilityCheckJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        tracing::info!(image_id = self.image_id, path = %self.image_path, "AccessibilityCheckJob started");
        let state = worker_state();
        let result = handlers::handle_accessibility_check(self.clone(), state).await;
        match &result {
            Ok(()) => tracing::info!(image_id = self.image_id, "AccessibilityCheckJob completed"),
            Err(e) => tracing::error!(image_id = self.image_id, error = %e, "AccessibilityCheckJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    fn task_type(&self) -> String {
        "accessibility_check".to_string()
    }

    fn max_retries(&self) -> i32 {
        3
    }

    fn backoff(&self, attempt: u32) -> u32 {
        u32::pow(2, attempt)
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for DiscoverJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        tracing::info!(hop = self.hop, "DiscoverJob started");
        let state = worker_state();
        let result = handlers::handle_discover(self.clone(), state).await;
        match &result {
            Ok(()) => tracing::info!(hop = self.hop, "DiscoverJob completed"),
            Err(e) => tracing::error!(hop = self.hop, error = %e, "DiscoverJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    fn task_type(&self) -> String {
        "discover".to_string()
    }

    fn max_retries(&self) -> i32 {
        3
    }

    fn backoff(&self, attempt: u32) -> u32 {
        u32::pow(2, attempt)
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for RefreshPixivTokenJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        tracing::info!(credential_id = self.credential_id, "RefreshPixivTokenJob started");
        let state = worker_state();
        let result = handlers::handle_refresh_pixiv_token(self.clone(), state).await;
        match &result {
            Ok(()) => tracing::info!(credential_id = self.credential_id, "RefreshPixivTokenJob completed"),
            Err(e) => tracing::error!(credential_id = self.credential_id, error = %e, "RefreshPixivTokenJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    fn task_type(&self) -> String {
        "refresh_pixiv_token".to_string()
    }

    fn max_retries(&self) -> i32 {
        3
    }

    fn backoff(&self, attempt: u32) -> u32 {
        u32::pow(2, attempt)
    }
}
