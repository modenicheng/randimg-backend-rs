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

use config::AppConfig;
use std::sync::Arc;
use task_queue::fang_backend::QueueBackend;

#[derive(Clone)]
pub struct WorkerState {
    pub db: sea_orm::DatabaseConnection,
    pub config: AppConfig,
    pub oss: dogecloud::DogeCloudOss,
    pub queue_backend: QueueBackend,
    pub http_client: reqwest::Client,
}

/// Spawn fang workers for all job types. Returns handles for graceful shutdown.
///
/// Uses fang's `AsyncWorkerPool` which auto-discovers `AsyncRunnable` impls
/// via typetag. Each worker type is registered with its own concurrency level
/// from `AppConfig`.
///
/// `worker_handle`: Tokio runtime handle to spawn workers on. This allows isolating
/// background workers from the HTTP runtime to prevent API starvation.
pub async fn spawn_workers(
    state: Arc<WorkerState>,
    worker_handle: tokio::runtime::Handle,
) -> Vec<tokio::task::JoinHandle<()>> {
    use fang::asynk::async_queue::AsyncQueue;
    use fang::asynk::async_worker_pool::AsyncWorkerPool;

    // Initialize the global WorkerState so AsyncRunnable::run() can access it
    task_queue::jobs::init_worker_state(state.clone()).await;

    let queue: AsyncQueue = state.queue_backend.queue().clone();
    let mut handles = Vec::new();

    // Create a worker pool — fang auto-discovers AsyncRunnable impls via typetag.
    // The pool polls the fang_tasks table and dispatches to the matching handler.
    let mut pool = AsyncWorkerPool::<AsyncQueue>::builder()
        .number_of_workers(2u32)
        .queue(queue)
        .build();

    tracing::info!("Spawning fang worker pool with 2 workers");

    let handle = worker_handle.spawn(async move {
        pool.start().await;
    });
    handles.push(handle);

    tracing::info!(count = handles.len(), "Worker pool spawned");
    handles
}
