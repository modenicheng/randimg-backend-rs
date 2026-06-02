use serde::{Deserialize, Serialize};

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
