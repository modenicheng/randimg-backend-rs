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
