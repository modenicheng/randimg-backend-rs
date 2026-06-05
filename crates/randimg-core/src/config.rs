use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Parsed bind address — supports TCP and Unix socket formats.
///
/// Accepted formats:
/// - `0.0.0.0:8000` / `127.0.0.1:8000` — plain TCP
/// - `http://127.0.0.1:8000` — TCP (scheme is stripped)
/// - `unix:///run/randimg.sock` — Unix domain socket
#[derive(Clone, Debug)]
pub enum BindAddr {
    Tcp(SocketAddr),
    Unix(PathBuf),
}

impl BindAddr {
    pub fn parse(addr: &str) -> Self {
        if let Some(path) = addr.strip_prefix("unix://") {
            return BindAddr::Unix(PathBuf::from(path));
        }

        // Strip URL scheme if present
        let addr = addr
            .strip_prefix("http://")
            .or_else(|| addr.strip_prefix("https://"))
            .unwrap_or(addr);

        match addr.parse::<SocketAddr>() {
            Ok(sa) => BindAddr::Tcp(sa),
            Err(_) => {
                // Try hostname resolution (e.g. "localhost:8000")
                use std::net::ToSocketAddrs;
                addr.to_socket_addrs()
                    .expect(&format!("Cannot resolve bind address '{}'", addr))
                    .next()
                    .map(BindAddr::Tcp)
                    .expect(&format!("No addresses resolved for '{}'", addr))
            }
        }
    }
}

#[derive(Clone)]
pub struct AppConfig {
    /// API 数据库 (SeaORM) — 存储业务数据和自定义任务表
    pub api_database_url: String,
    /// 队列数据库 (Fang) — 存储 fang_tasks 表
    pub queue_database_url: String,
    pub secret_key: String,
    pub jwt_expire_minutes: u64,
    pub cdn_base_url: String,
    pub image_dir: String,
    pub server_addr: BindAddr,
    pub pixiv_refresh_token: String,
    pub pixiv_proxy: String,
    pub pixiv_accept_lang: String,
    pub log_level: String,
    pub log_dir: String,
    pub log_json: bool,
    pub max_discover_hops: u32,
    pub discover_seed_limit: u64,
    // DogeCloud OSS
    pub dogecloud_access_key: String,
    pub dogecloud_secret_key: String,
    pub dogecloud_s3_bucket: String,
    pub dogecloud_s3_endpoint: String,
    // Color worker process isolation
    pub color_worker_rayon_threads: usize,
    pub color_worker_standalone: bool,
    // Color extraction parameters
    pub color_extract_k: usize,
    pub color_extract_max_iter: usize,
    pub color_extract_batch_size: usize,
    pub color_extract_image_scale: f64,
    // CORS
    pub cors_origins: String,

    // ── Fang 任务调度配置 ─────────────────────────────────────
    /// 最大重试次数（Fang 任务失败后重试）
    pub task_max_retries: i32,
    /// 退避基数（指数退避：base^n 秒）
    pub task_backoff_base: u32,
    /// 轮询间隔（毫秒）— Fang worker 检查新任务的频率
    pub task_poll_interval_ms: u64,
    /// 默认超时时间（秒）— 超过此时间未完成的任务将被标记为失败
    pub task_default_timeout_secs: u64,

    // ── 各任务类型并发数 ─────────────────────────────────────
    pub task_concurrency_crawl: u32,
    pub task_concurrency_download: u32,
    pub task_concurrency_color_extract: u32,
    pub task_concurrency_upload: u32,
    pub task_concurrency_accessibility_check: u32,
    pub task_concurrency_discover: u32,
    pub task_concurrency_refresh_pixiv_token: u32,
}

impl std::fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppConfig")
            .field("api_database_url", &"[REDACTED]")
            .field("queue_database_url", &"[REDACTED]")
            .field("secret_key", &"[REDACTED]")
            .field("jwt_expire_minutes", &self.jwt_expire_minutes)
            .field("cdn_base_url", &self.cdn_base_url)
            .field("image_dir", &self.image_dir)
            .field("server_addr", &self.server_addr)
            .field("pixiv_refresh_token", &"[REDACTED]")
            .field("pixiv_proxy", &self.pixiv_proxy)
            .field("pixiv_accept_lang", &self.pixiv_accept_lang)
            .field("log_level", &self.log_level)
            .field("log_dir", &self.log_dir)
            .field("log_json", &self.log_json)
            .field("max_discover_hops", &self.max_discover_hops)
            .field("discover_seed_limit", &self.discover_seed_limit)
            .field("dogecloud_access_key", &"[REDACTED]")
            .field("dogecloud_secret_key", &"[REDACTED]")
            .field("dogecloud_s3_bucket", &self.dogecloud_s3_bucket)
            .field("dogecloud_s3_endpoint", &self.dogecloud_s3_endpoint)
            .field("color_worker_rayon_threads", &self.color_worker_rayon_threads)
            .field("color_worker_standalone", &self.color_worker_standalone)
            .field("color_extract_k", &self.color_extract_k)
            .field("color_extract_max_iter", &self.color_extract_max_iter)
            .field("color_extract_batch_size", &self.color_extract_batch_size)
            .field("color_extract_image_scale", &self.color_extract_image_scale)
            .field("cors_origins", &self.cors_origins)
            .field("task_max_retries", &self.task_max_retries)
            .field("task_backoff_base", &self.task_backoff_base)
            .field("task_poll_interval_ms", &self.task_poll_interval_ms)
            .field("task_default_timeout_secs", &self.task_default_timeout_secs)
            .field("task_concurrency_crawl", &self.task_concurrency_crawl)
            .field("task_concurrency_download", &self.task_concurrency_download)
            .field("task_concurrency_color_extract", &self.task_concurrency_color_extract)
            .field("task_concurrency_upload", &self.task_concurrency_upload)
            .field("task_concurrency_accessibility_check", &self.task_concurrency_accessibility_check)
            .field("task_concurrency_discover", &self.task_concurrency_discover)
            .field("task_concurrency_refresh_pixiv_token", &self.task_concurrency_refresh_pixiv_token)
            .finish()
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        let secret_key = env::var("SECRET_KEY").unwrap_or_else(|_| "change-me".into());
        if secret_key == "change-me" {
            panic!("SECRET_KEY must be set in environment. Do not use the default 'change-me'.");
        }

        Self {
            api_database_url: env::var("API_DATABASE_URL")
                .unwrap_or_else(|_| "postgres://localhost/randimg".into()),
            queue_database_url: env::var("QUEUE_DATABASE_URL")
                .unwrap_or_else(|_| "postgres://localhost/randimg_queue".into()),
            secret_key,
            jwt_expire_minutes: env::var("JWT_EXPIRE_MINUTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            cdn_base_url: env::var("CDN_BASE_URL")
                .unwrap_or_else(|_| "https://cdn.example.com/".into()),
            image_dir: env::var("IMAGE_DIR").unwrap_or_else(|_| "./images".into()),
            server_addr: BindAddr::parse(
                &env::var("SERVER_ADDR").unwrap_or_else(|_| "0.0.0.0:8000".into()),
            ),
            pixiv_refresh_token: env::var("PIXIV_REFRESH_TOKEN").unwrap_or_default(),
            pixiv_proxy: env::var("PIXIV_PROXY").unwrap_or_default(),
            pixiv_accept_lang: env::var("PIXIV_ACCEPT_LANG")
                .unwrap_or_else(|_| "zh-CN".into()),
            log_level: env::var("RUST_LOG")
                .unwrap_or_else(|_| "info,randimg_core=debug,tower_http=info,fang=debug".into()),
            log_dir: env::var("LOG_DIR").unwrap_or_else(|_| "./logs".into()),
            log_json: env::var("LOG_JSON")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            max_discover_hops: env::var("MAX_DISCOVER_HOPS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            discover_seed_limit: env::var("DISCOVER_SEED_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            dogecloud_access_key: env::var("DOGECLOUD_ACCESS_KEY").unwrap_or_default(),
            dogecloud_secret_key: env::var("DOGECLOUD_SECRET_KEY").unwrap_or_default(),
            dogecloud_s3_bucket: env::var("DOGECLOUD_S3_BUCKET").unwrap_or_default(),
            dogecloud_s3_endpoint: env::var("DOGECLOUD_S3_ENDPOINT").unwrap_or_default(),
            color_worker_rayon_threads: env::var("COLOR_WORKER_RAYON_THREADS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| {
                    std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(4)
                }),
            color_worker_standalone: env::var("COLOR_WORKER_STANDALONE")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false),
            color_extract_k: env::var("COLOR_EXTRACT_K")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            color_extract_max_iter: env::var("COLOR_EXTRACT_MAX_ITER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50),
            color_extract_batch_size: env::var("COLOR_EXTRACT_BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2048),
            color_extract_image_scale: env::var("COLOR_EXTRACT_IMAGE_SCALE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.5),
            cors_origins: env::var("CORS_ORIGINS").unwrap_or_else(|_| "*".into()),

            // Fang 任务调度配置
            task_max_retries: env::var("TASK_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            task_backoff_base: env::var("TASK_BACKOFF_BASE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2),
            task_poll_interval_ms: env::var("TASK_POLL_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(500),
            task_default_timeout_secs: env::var("TASK_DEFAULT_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),

            // 各任务类型并发数
            task_concurrency_crawl: env::var("TASK_CONCURRENCY_CRAWL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2),
            task_concurrency_download: env::var("TASK_CONCURRENCY_DOWNLOAD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4),
            task_concurrency_color_extract: env::var("TASK_CONCURRENCY_COLOR_EXTRACT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2),
            task_concurrency_upload: env::var("TASK_CONCURRENCY_UPLOAD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2),
            task_concurrency_accessibility_check: env::var("TASK_CONCURRENCY_ACCESSIBILITY_CHECK")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2),
            task_concurrency_discover: env::var("TASK_CONCURRENCY_DISCOVER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
            task_concurrency_refresh_pixiv_token: env::var("TASK_CONCURRENCY_REFRESH_PIXIV_TOKEN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
        }
    }
}
