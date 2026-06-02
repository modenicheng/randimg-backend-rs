use axum::Router;
use axum::http::HeaderValue;
use randimg_backend_rs::config::{AppConfig, BindAddr};
use randimg_backend_rs::task_queue::jobs::*;
use randimg_backend_rs::{AppState, db, db::query, db_backend, handlers};
use std::sync::Arc;
use tokio::signal;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    let config = AppConfig::from_env();

    // --- Logging setup -------------------------------------------------------
    let file_appender_guard = if config.log_json {
        let file_appender = rolling::daily(&config.log_dir, "randimg.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        tracing_subscriber::registry()
            .with(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new(&config.log_level)),
            )
            .with(tracing_subscriber::fmt::layer().json())
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(non_blocking)
                    .json(),
            )
            .init();
        guard
    } else {
        let file_appender = rolling::daily(&config.log_dir, "randimg.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        tracing_subscriber::registry()
            .with(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new(&config.log_level)),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .with_file(true)
                    .with_line_number(true)
                    .with_thread_ids(true)
                    .with_target(true)
                    .compact(),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(non_blocking)
                    .json(),
            )
            .init();
        guard
    };
    let _appender_guard = file_appender_guard;

    let db = db::init_database(&config.database_url).await;

    // Build a shared reqwest::Client with proxy and timeout.
    // All HTTP requests (Pixiv downloads, DogeCloud API, etc.) reuse this single client
    // for connection pooling and consistent configuration.
    let mut http_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("PixivAndroidApp/5.0.234 (Android 11; Pixel 5)");

    if !config.pixiv_proxy.is_empty() {
        if let Ok(proxy) = reqwest::Proxy::all(&config.pixiv_proxy) {
            http_builder = http_builder.proxy(proxy);
        }
    }

    let http_client = http_builder
        .build()
        .expect("Failed to build shared HTTP client");

    let oss = randimg_backend_rs::dogecloud::DogeCloudOss::new(&config, http_client.clone());

    // --- Apalis job queue setup ------------------------------------------------
    let (apalis_pool, job_storage) = db_backend::init(&config.database_url)
        .await
        .expect("Failed to initialize Apalis job queue");

    let state = Arc::new(AppState {
        db,
        config: config.clone(),
        oss,
        job_storage,
        apalis_pool: apalis_pool.clone(),
        http_client,
        worker_handles: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    });

    // --- Pixiv credential seed & auto-refresh ----------------------------------
    // Seed from env var if DB has no credentials yet
    if !config.pixiv_refresh_token.is_empty() {
        let existing = query::pixiv_credential::find_all(&state.db)
            .await
            .unwrap_or_default();
        if existing.is_empty() {
            match query::pixiv_credential::create(
                &state.db,
                "env_default",
                &config.pixiv_refresh_token,
                Some("Seeded from PIXIV_REFRESH_TOKEN env var"),
            )
            .await
            {
                Ok(cred) => {
                    tracing::info!(cred_id = cred.id, "Seeded Pixiv credential from env var");
                }
                Err(e) => {
                    tracing::error!("Failed to seed Pixiv credential: {}", e);
                }
            }
        }
    }

    // Submit a refresh task for active credentials with stale tokens
    // (skip if token was refreshed within the last 50 minutes)
    let stale_threshold = chrono::Utc::now().naive_utc() - chrono::Duration::minutes(50);
    match query::pixiv_credential::find_all(&state.db).await {
        Ok(creds) => {
            for cred in creds.iter().filter(|c| c.status == 0) {
                let needs_refresh = cred
                    .last_refreshed_at
                    .map(|t| t < stale_threshold)
                    .unwrap_or(true);
                if !needs_refresh {
                    tracing::info!(
                        cred_id = cred.id,
                        "Pixiv token still fresh, skipping refresh"
                    );
                    continue;
                }
                if let Err(e) = state
                    .job_storage
                    .push_refresh_pixiv_token(RefreshPixivTokenJob {
                        credential_id: cred.id,
                        parent_job_id: None,
                    })
                    .await
                {
                    tracing::error!(cred_id = cred.id, "Failed to submit refresh task: {}", e);
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to list credentials for auto-refresh: {}", e);
        }
    }

    // --- Apalis workers -------------------------------------------------------
    let worker_handles = randimg_backend_rs::spawn_workers(state.clone(), &apalis_pool).await;
    *state.worker_handles.lock().await = worker_handles;

    // --- Optional: spawn color-worker as child process -----------------------
    let mut color_worker_child: Option<tokio::process::Child> = if config.color_worker_standalone
        && std::env::var("COLOR_WORKER_AUTO_SPAWN")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false)
    {
        tracing::info!("Spawning color-worker as child process");
        let exe = std::env::current_exe().expect("Failed to get current exe path");
        let child = tokio::process::Command::new(exe)
            .arg("color-worker")
            .env("DATABASE_URL", &config.database_url)
            .env("SECRET_KEY", &config.secret_key)
            .env(
                "COLOR_WORKER_RAYON_THREADS",
                config.color_worker_rayon_threads.to_string(),
            )
            .env("RUST_LOG", &config.log_level)
            .spawn()
            .expect("Failed to spawn color-worker child process");
        tracing::info!(pid = ?child.id(), "Color-worker child process spawned");
        Some(child)
    } else {
        None
    };

    let cors = if state.config.cors_origins == "*" {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins: Vec<HeaderValue> = state
            .config
            .cors_origins
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    let app = Router::new()
        .merge(handlers::health::routes())
        .merge(handlers::image::routes())
        .merge(handlers::auth::routes())
        .merge(handlers::tag::routes())
        .merge(handlers::statistic::routes())
        .merge(handlers::author::routes())
        .merge(handlers::crawler::routes())
        .merge(handlers::task::routes())
        .merge(handlers::pixiv_credential::routes())
        .nest_service("/images", ServeDir::new(&config.image_dir))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state.clone());

    match &config.server_addr {
        BindAddr::Tcp(addr) => {
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            tracing::info!(
                address = %listener.local_addr().unwrap(),
                database = %config.database_url,
                "Server started (TCP)"
            );
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
                .unwrap();
        }
        BindAddr::Unix(path) => {
            // Remove stale socket file if it exists
            if path.exists() {
                std::fs::remove_file(path).unwrap_or_else(|e| {
                    tracing::warn!("Failed to remove stale socket {:?}: {}", path, e);
                });
            }
            let listener = tokio::net::UnixListener::bind(path).unwrap();
            tracing::info!(
                socket = %path.display(),
                database = %config.database_url,
                "Server started (Unix socket)"
            );
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
                .unwrap();
        }
    }

    // Shut down color-worker child process if spawned
    if let Some(ref mut child) = color_worker_child {
        tracing::info!("Sending kill signal to color-worker child process");
        child.kill().await.ok();
        let _ = child.wait().await;
    }

    tracing::info!("Shutting down — aborting Apalis workers…");
    let handles = {
        let mut guard = state.worker_handles.lock().await;
        std::mem::take(&mut *guard)
    };
    for h in &handles {
        h.abort();
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        for h in handles {
            let _ = h.await;
        }
    })
    .await;

    tracing::info!("Shutdown complete");
}

async fn shutdown_signal() {
    let ctrl_c = signal::ctrl_c();
    let sigterm = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received SIGINT (CTRL+C)");
        }
        _ = sigterm => {
            tracing::info!("Received SIGTERM");
        }
    }
}
