//! Standalone color extraction worker process.
//!
//! This binary runs only the color_extract Apalis worker, connecting to the
//! same database as the main server. It uses its own tokio runtime and a
//! dedicated rayon thread pool to avoid competing with the main process for
//! CPU resources.
//!
//! ## Usage
//!
//! ```bash
//! # Run as a separate process (recommended for production)
//! COLOR_WORKER_RAYON_THREADS=4 cargo run --bin color-worker
//!
//! # Or with the release binary
//! COLOR_WORKER_RAYON_THREADS=4 ./target/release/color-worker
//! ```
//!
//! ## Environment Variables
//!
//! - `DATABASE_URL` — same as the main server
//! - `SECRET_KEY` — required (same as main server, used by AppState)
//! - `COLOR_WORKER_RAYON_THREADS` — rayon thread count (default: CPU count)
//! - `LOG_LEVEL` — tracing filter (default: `info`)

use apalis::layers::retry::RetryPolicy;
use apalis::prelude::*;
use randimg_backend_rs::config::AppConfig;
use randimg_backend_rs::task_queue::handlers::handle_color_extract;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    // Logging setup
    let log_level = std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level)),
        )
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(true)
        .compact()
        .init();

    let config = AppConfig::from_env();

    // Initialize the dedicated rayon pool BEFORE connecting to DB,
    // so the pool is ready when the first job arrives.
    let rayon_threads = config.color_worker_rayon_threads;
    randimg_backend_rs::color::init_color_pool(rayon_threads);
    tracing::info!(threads = rayon_threads, "Initialized color extraction rayon pool");

    // Connect to the same Apalis storage as the main server
    let (apalis_pool, job_storage) =
        randimg_backend_rs::db_backend::init(&config.database_url)
            .await
            .expect("Failed to initialize Apalis job queue");

    // Build a minimal AppState (only what handle_color_extract needs)
    let db = randimg_backend_rs::db::init_database(&config.database_url).await;

    let state = Arc::new(randimg_backend_rs::AppState {
        db,
        config: config.clone(),
        oss: randimg_backend_rs::dogecloud::DogeCloudOss::new_noop(),
        job_storage: job_storage.clone(),
        apalis_pool: apalis_pool.clone(),
        http_client: reqwest::Client::new(),
        worker_handles: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    });

    // Clone the color_extract storage for the worker
    let storage = job_storage.color_extract.lock().await.clone();

    tracing::info!("Starting color-worker process");

    let worker = WorkerBuilder::new("color-extract")
        .backend(storage)
        .data(state.clone() as Arc<randimg_backend_rs::AppState>)
        .data(job_storage.clone())
        .concurrency(2)
        .retry(RetryPolicy::retries(3))
        .enable_tracing()
        .build(handle_color_extract);

    // Run until shutdown signal
    tokio::select! {
        result = worker.run() => {
            match result {
                Ok(()) => tracing::warn!("Color worker exited unexpectedly"),
                Err(e) => tracing::error!(error = %e, "Color worker exited with error"),
            }
        }
        _ = shutdown_signal() => {
            tracing::info!("Received shutdown signal, stopping color worker");
        }
    }
}

async fn shutdown_signal() {
    use tokio::signal;
    let ctrl_c = signal::ctrl_c();
    let sigterm = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => tracing::info!("Received SIGINT"),
        _ = sigterm => tracing::info!("Received SIGTERM"),
    }
}
