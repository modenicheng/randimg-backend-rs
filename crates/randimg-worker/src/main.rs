//! Headless worker binary for Randimg background job processing.
//!
//! Runs all 7 Fang workers (crawl, download, color-extract, upload,
//! accessibility-check, discover, refresh-pixiv-token) plus a watchdog
//! task. No HTTP server — just background processing.
//!
//! ## Usage
//!
//! ```bash
//! cargo run -p randimg-worker
//! ```

use randimg_core::config::AppConfig;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    // 1. Load configuration
    let config = AppConfig::from_env();

    // 2. Set up logging (simpler than server — env filter + compact format)
    init_logging(&config);

    tracing::info!("Starting randimg-worker");

    // 3. Initialize dedicated rayon pool for color extraction
    randimg_core::color::init_color_pool(config.color_worker_rayon_threads);
    tracing::info!(
        threads = config.color_worker_rayon_threads,
        "Initialized color extraction rayon pool"
    );

    // 4. Init database (SeaORM + migrations)
    let db = randimg_core::db::init_database(&config.database_url).await;
    tracing::info!("Database connected and migrations applied");

    // 5. Build HTTP client with proxy support and 60s timeout
    let http_client = {
        let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(60));

        if !config.pixiv_proxy.is_empty() {
            match reqwest::Proxy::all(&config.pixiv_proxy) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                    tracing::info!(proxy = %config.pixiv_proxy, "HTTP client configured with proxy");
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to configure HTTP proxy, using direct connection");
                }
            }
        }

        builder.build().expect("Failed to build HTTP client")
    };

    // 6. Init DogeCloud OSS client
    let oss = randimg_core::dogecloud::DogeCloudOss::new(&config, http_client.clone());
    tracing::info!("DogeCloud OSS client initialized");

    // 7. Init Fang job queue
    let queue_backend = randimg_core::db_backend::init(&config)
        .await
        .expect("Failed to initialize Fang job queue");
    tracing::info!("Fang job queue initialized");

    // 8. Construct WorkerState
    let state = Arc::new(randimg_core::WorkerState {
        db,
        config: config.clone(),
        oss,
        queue_backend,
        http_client,
    });

    let handles = randimg_core::spawn_workers(
        state.clone(),
        tokio::runtime::Handle::current(),
    )
    .await;

    let worker_handles = Arc::new(Mutex::new(handles));
    tracing::info!(
        count = worker_handles.lock().await.len(),
        "All workers spawned"
    );

    shutdown_signal().await;
    tracing::info!("Shutdown signal received, aborting workers...");

    let handles = worker_handles.lock().await;
    for handle in handles.iter() {
        handle.abort();
    }
    tracing::info!(count = handles.len(), "All workers aborted, exiting");
}

/// Initialize tracing with env filter and compact format.
fn init_logging(config: &AppConfig) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(true)
        .compact();

    // Set up file appender if log_dir is configured
    if !config.log_dir.is_empty() {
        let file_appender = tracing_appender::rolling::daily(&config.log_dir, "worker.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

        // We need to leak the guard so it lives for the program duration
        // This is intentional for the logging use case
        Box::leak(Box::new(_guard));

        subscriber.with_writer(non_blocking).init();
    } else {
        subscriber.init();
    }
}

/// Wait for SIGINT (Ctrl+C) or SIGTERM.
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
