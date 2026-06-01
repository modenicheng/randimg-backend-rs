pub mod auth;
pub mod color;
pub mod config;
pub mod db;
pub mod db_backend;
pub mod dogecloud;
pub mod error;
pub mod handlers;
pub mod pixiv;
pub mod task_queue;

use apalis::layers::retry::RetryPolicy;
use apalis::prelude::*;
use config::AppConfig;
use std::sync::Arc;
use task_queue::handlers::*;

/// The Apalis connection pool type — determined by feature flag.
pub type ApalisPool = db_backend::Pool;

#[derive(Clone)]
pub struct AppState {
    pub db: sea_orm::DatabaseConnection,
    pub config: AppConfig,
    pub oss: dogecloud::DogeCloudOss,
    pub job_storage: db_backend::JobStorage,
    pub apalis_pool: ApalisPool,
    /// Worker JoinHandles for runtime management (abort/cleanup).
    /// Wrapped in Arc<tokio::sync::Mutex> because JoinHandle is not Clone
    /// and we need shared mutable access from HTTP handlers.
    pub worker_handles: Arc<tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

/// Spawn Apalis workers for all job types. Returns handles for graceful shutdown.
pub async fn spawn_workers(
    state: Arc<AppState>,
    _pool: &ApalisPool,
) -> Vec<tokio::task::JoinHandle<()>> {
    let js = &state.job_storage;

    // Helper macro to build and spawn a worker.
    // Clones the storage out of the Mutex (shares the same pool).
    macro_rules! spawn_worker {
        ($name:expr, $storage:expr, $handler:expr, $concurrency:expr) => {{
            let storage = $storage.lock().await.clone();
            let state = state.clone();
            let js_clone = js.clone();
            let handle = tokio::spawn(async move {
                let worker = WorkerBuilder::new($name)
                    .backend(storage)
                    .data(state.clone())
                    .data(js_clone.clone())
                    .concurrency($concurrency)
                    .retry(RetryPolicy::retries(3))
                    .enable_tracing()
                    .build($handler);
                worker.run().await.ok();
            });
            handle
        }};
    }

    vec![
        spawn_worker!("crawl", js.crawl, handle_crawl, 2),
        spawn_worker!("download", js.download, handle_download, 4),
        spawn_worker!("color-extract", js.color_extract, handle_color_extract, 2),
        spawn_worker!("upload", js.upload, handle_upload, 2),
        spawn_worker!(
            "accessibility-check",
            js.accessibility_check,
            handle_accessibility_check,
            2
        ),
        spawn_worker!("discover", js.discover, handle_discover, 1),
        spawn_worker!(
            "refresh-pixiv-token",
            js.refresh_pixiv_token,
            handle_refresh_pixiv_token,
            1
        ),
    ]
}
