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

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub database_url: String,
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
    // Retry backoff
    pub retry_max_retries: usize,
    pub retry_backoff_min_ms: u64,
    pub retry_backoff_max_secs: u64,
    pub retry_backoff_jitter: f64,
    // DogeCloud OSS
    pub dogecloud_access_key: String,
    pub dogecloud_secret_key: String,
    pub dogecloud_s3_bucket: String,
    pub dogecloud_s3_endpoint: String,
    // Color worker process isolation
    pub color_worker_rayon_threads: usize,
    pub color_worker_standalone: bool,
}

impl AppConfig {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        let secret_key = env::var("SECRET_KEY").unwrap_or_else(|_| "change-me".into());
        if secret_key == "change-me" {
            panic!("SECRET_KEY must be set in environment. Do not use the default 'change-me'.");
        }

        Self {
            database_url: {
                let url = env::var("DATABASE_URL")
                    .unwrap_or_else(|_| "sqlite://data/randimg.db?mode=rwc".into());
                #[cfg(feature = "sqlite")]
                if url.starts_with("postgres://") || url.starts_with("postgresql://") {
                    panic!("Compiled with sqlite feature but DATABASE_URL is a PostgreSQL URL. Rebuild with --features postgres --no-default-features.");
                }
                #[cfg(feature = "postgres")]
                if url.starts_with("sqlite://") {
                    panic!("Compiled with postgres feature but DATABASE_URL is a SQLite URL. Rebuild with --features sqlite (default).");
                }
                url
            },
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
                .unwrap_or_else(|_| "info,randimg_backend_rs=debug,tower_http=info,apalis=debug".into()),
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
            // Retry backoff
            retry_max_retries: env::var("RETRY_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            retry_backoff_min_ms: env::var("RETRY_BACKOFF_MIN_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000),
            retry_backoff_max_secs: env::var("RETRY_BACKOFF_MAX_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            retry_backoff_jitter: env::var("RETRY_BACKOFF_JITTER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.5),
            // DogeCloud OSS
            dogecloud_access_key: env::var("DOGECLOUD_ACCESS_KEY").unwrap_or_default(),
            dogecloud_secret_key: env::var("DOGECLOUD_SECRET_KEY").unwrap_or_default(),
            dogecloud_s3_bucket: env::var("DOGECLOUD_S3_BUCKET").unwrap_or_default(),
            dogecloud_s3_endpoint: env::var("DOGECLOUD_S3_ENDPOINT").unwrap_or_default(),
            // Color worker process isolation
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
        }
    }
}
