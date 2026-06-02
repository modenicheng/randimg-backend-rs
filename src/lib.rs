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
    /// Shared HTTP client — configured once with proxy, timeout, and connection pooling.
    /// Used by all handlers (download, DogeCloud API, etc.) instead of creating new clients.
    pub http_client: reqwest::Client,
    /// Worker JoinHandles for runtime management (abort/cleanup).
    /// Wrapped in Arc<tokio::sync::Mutex> because JoinHandle is not Clone
    /// and we need shared mutable access from HTTP handlers.
    pub worker_handles: Arc<tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

/// Spawn Apalis workers for all job types. Returns handles for graceful shutdown.
///
/// Each worker runs in a restart loop: if it exits (error or unexpected Ok),
/// it logs the failure and restarts with exponential backoff (2s → 4s → … → 30s cap).
/// A watchdog task periodically checks `Workers.last_seen` in the database and
/// logs alerts when a worker's heartbeat goes stale.
pub async fn spawn_workers(
    state: Arc<AppState>,
    _pool: &ApalisPool,
) -> Vec<tokio::task::JoinHandle<()>> {
    let js = &state.job_storage;

    // Helper macro to build and spawn a worker with automatic restart on failure.
    // On each restart iteration the storage is re-cloned from the shared Mutex
    // so the worker gets a fresh backend connection.
    macro_rules! spawn_worker {
        ($name:expr, $storage:expr, $handler:expr, $concurrency:expr) => {{
            let storage_arc = $storage.clone(); // Arc<Mutex<Storage<T>>>
            let state = state.clone();
            let js_clone = js.clone();
            let handle = tokio::spawn(async move {
                let mut attempt = 0u32;
                loop {
                    attempt += 1;
                    let storage = storage_arc.lock().await.clone();

                    tracing::info!(worker = $name, attempt, "Starting worker");

                    let worker = WorkerBuilder::new($name)
                        .backend(storage)
                        .data(state.clone())
                        .data(js_clone.clone())
                        .concurrency($concurrency)
                        .retry(RetryPolicy::retries(3))
                        .enable_tracing()
                        .build($handler);

                    match worker.run().await {
                        Ok(()) => {
                            tracing::warn!(
                                worker = $name,
                                attempt,
                                "Worker exited unexpectedly (returned Ok)"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                worker = $name,
                                attempt,
                                error = %e,
                                "Worker exited with error"
                            );
                        }
                    }

                    let backoff = std::cmp::min(attempt * 2, 30);
                    tracing::warn!(
                        worker = $name,
                        attempt,
                        backoff_secs = backoff,
                        "Restarting worker after backoff"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(backoff as u64)).await;
                }
            });
            handle
        }};
    }

    let mut handles = vec![
        spawn_worker!("crawl", js.crawl, handle_crawl, 2),
        spawn_worker!("download", js.download, handle_download, 4),
        // color-extract worker: skip if running as standalone process
        if state.config.color_worker_standalone {
            tracing::info!("Skipping color-extract worker (COLOR_WORKER_STANDALONE=true)");
            tokio::spawn(async { std::future::pending::<()>().await })
        } else {
            spawn_worker!("color-extract", js.color_extract, handle_color_extract, 2)
        },
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
    ];

    // Watchdog: periodically probe the Workers table for stale heartbeats.
    // A worker whose `last_seen` is >STALE_THRESHOLD seconds behind is
    // almost certainly dead or stuck — the restart loop inside the worker
    // task should have caught the crash, so a stale heartbeat here means
    // the task itself panicked or was cancelled externally.
    let watchdog_state = state.clone();
    handles.push(tokio::spawn(async move {
        const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
        const WARN_THRESHOLD: i64 = 60;
        const STALE_THRESHOLD: i64 = 120;

        let mut interval = tokio::time::interval(CHECK_INTERVAL);
        loop {
            interval.tick().await;
            check_worker_health(&watchdog_state, WARN_THRESHOLD, STALE_THRESHOLD).await;
        }
    }));

    handles
}

/// Query the `Workers` table and log warnings/errors for stale heartbeats.
async fn check_worker_health(state: &AppState, warn_secs: i64, stale_secs: i64) {
    #[cfg(feature = "sqlite")]
    {
        use sea_orm::{ConnectionTrait, Statement};

        let result = state
            .db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT worker_type, \
                 (strftime('%s', 'now') - last_seen) AS age_secs \
                 FROM Workers"
                    .to_string(),
            ))
            .await;

        match result {
            Ok(rows) => {
                for row in rows {
                    let worker_type: String =
                        row.try_get("", "worker_type").unwrap_or_default();
                    let age: i64 = row.try_get("", "age_secs").unwrap_or(0);

                    if age > stale_secs {
                        tracing::error!(
                            worker = %worker_type,
                            age_secs = age,
                            "⚠ Worker heartbeat stale — task may have panicked or been cancelled"
                        );
                    } else if age > warn_secs {
                        tracing::warn!(
                            worker = %worker_type,
                            age_secs = age,
                            "Worker heartbeat delayed"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Watchdog: failed to query Workers table");
            }
        }
    }
}
