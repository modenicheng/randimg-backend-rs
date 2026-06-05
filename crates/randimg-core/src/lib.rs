#[cfg(feature = "http")]
pub mod auth;
pub mod color;
pub mod config;
pub mod db;
pub mod db_backend;
pub mod dogecloud;
#[cfg(feature = "http")]
pub mod error;
#[cfg(feature = "http")]
pub mod handlers;
pub mod pixiv;
pub mod task_queue;
pub mod watchdog;

use config::AppConfig;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};
use task_queue::fang_backend::QueueBackend;
use task_queue::fingerprint::FingerprintCache;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct WorkerState {
    pub db: sea_orm::DatabaseConnection,
    pub config: AppConfig,
    pub oss: dogecloud::DogeCloudOss,
    pub queue_backend: QueueBackend,
    pub http_client: reqwest::Client,
    pub shutdown_token: CancellationToken,
    pub worker_start_time: Instant,
    pub active_tasks: Arc<AtomicUsize>,
    pub discover_cache: Arc<DashMap<String, Instant>>,
    pub fingerprint_cache: Arc<FingerprintCache>,
    pub last_activity: Arc<DashMap<String, Instant>>,
    pub stuck_pools: Arc<DashMap<String, Instant>>,
}

/// Spawn fang workers for all job types. Returns handles for graceful shutdown.
///
/// Creates one `AsyncWorkerPool` per task type, each with its own concurrency level
/// from `AppConfig`. The `.task_type()` setting ensures each pool only polls tasks
/// matching its type string (e.g. "crawl", "download", "color_extract").
///
/// `worker_handle`: Tokio runtime handle to spawn workers on. This allows isolating
/// background workers from the HTTP runtime to prevent API starvation.
pub async fn spawn_workers(
    state: Arc<WorkerState>,
    worker_handle: tokio::runtime::Handle,
) -> Vec<tokio::task::JoinHandle<()>> {
    use fang::asynk::async_queue::AsyncQueue;
    use fang::asynk::async_worker_pool::AsyncWorkerPool;
    use fang::SleepParams;

    // Initialize the global WorkerState so AsyncRunnable::run() can access it
    task_queue::jobs::init_worker_state(state.clone()).await;

    let queue: AsyncQueue = state.queue_backend.queue().clone();
    let mut handles = Vec::new();

    let sleep_params = SleepParams {
        sleep_period: Duration::from_millis(state.config.task_poll_interval_ms),
        min_sleep_period: Duration::from_millis(state.config.task_poll_interval_ms),
        max_sleep_period: Duration::from_millis(state.config.task_poll_interval_ms * 3),
        sleep_step: Duration::from_millis(state.config.task_poll_interval_ms / 5),
    };

    let pool_configs: &[(&str, u32)] = &[
        ("crawl", state.config.task_concurrency_crawl),
        ("download", state.config.task_concurrency_download),
        ("color_extract", state.config.task_concurrency_color_extract),
        ("upload", state.config.task_concurrency_upload),
        ("accessibility_check", state.config.task_concurrency_accessibility_check),
        ("discover", state.config.task_concurrency_discover),
        ("refresh_pixiv_token", state.config.task_concurrency_refresh_pixiv_token),
        ("cleanup", state.config.task_concurrency_cleanup),
    ];

    for &(task_type, concurrency) in pool_configs {
        let mut pool = AsyncWorkerPool::<AsyncQueue>::builder()
            .number_of_workers(concurrency)
            .task_type(task_type)
            .queue(queue.clone())
            .sleep_params(sleep_params.clone())
            .retention_mode(fang::RetentionMode::KeepAll)
            .build();

        tracing::info!(
            task_type,
            concurrency,
            "Spawning fang worker pool"
        );

        let handle = worker_handle.spawn(async move {
            pool.start().await;
        });
        handles.push(handle);
    }

    tracing::info!(count = handles.len(), "Worker pools spawned");
    handles
}
