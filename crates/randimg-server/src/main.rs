use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::HeaderValue;
use randimg_core::config::{AppConfig, BindAddr};
use randimg_core::{WorkerState, db, db::query, db_backend, handlers};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tokio::signal;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Server-specific state wrapping WorkerState with HTTP layer fields.
#[derive(Clone)]
pub struct ServerState {
    pub worker: WorkerState,
}

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

    let db = db::init_database(&config.api_database_url).await;

    // Build a shared reqwest::Client with proxy and timeout.
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

    let oss = randimg_core::dogecloud::DogeCloudOss::new(&config, http_client.clone());

    // --- Fang job queue setup --------------------------------------------------
    // Server needs queue access to push jobs (e.g. refresh-pixiv-token on startup).
    // Workers run in the separate randimg-worker binary.
    let queue_backend = db_backend::init(&config)
        .await
        .expect("Failed to initialize fang job queue");

    let state = Arc::new(ServerState {
        worker: WorkerState {
            db,
            config: config.clone(),
            oss,
            queue_backend: queue_backend.clone(),
            http_client,
            shutdown_token: tokio_util::sync::CancellationToken::new(),
            worker_start_time: std::time::Instant::now(),
            active_tasks: Arc::new(AtomicUsize::new(0)),
            discover_cache: Arc::new(dashmap::DashMap::new()),
            fingerprint_cache: queue_backend.fingerprint_cache.clone(),
            last_activity: Arc::new(dashmap::DashMap::new()),
            stuck_pools: Arc::new(dashmap::DashMap::new()),
        },
    });

    // --- Pixiv credential seed & auto-refresh ----------------------------------
    // Seed from env var if DB has no credentials yet
    if !config.pixiv_refresh_token.is_empty() {
        let existing = query::pixiv_credential::find_all(&state.worker.db)
            .await
            .unwrap_or_default();
        if existing.is_empty() {
            match query::pixiv_credential::create(
                &state.worker.db,
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
    let stale_threshold = chrono::Utc::now().naive_utc() - chrono::Duration::minutes(50);
    match query::pixiv_credential::find_all(&state.worker.db).await {
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
                let refresh_task_id = uuid::Uuid::new_v4().to_string();
                if let Err(e) = state
                    .worker
                    .queue_backend
                    .push_task(
                        &randimg_core::task_queue::jobs::RefreshPixivTokenJob {
                            credential_id: cred.id,
                            parent_job_id: None,
                            task_id: Some(refresh_task_id.clone()),
                            max_retries: state.worker.config.task_max_retries,
                            backoff_base: state.worker.config.task_backoff_base,
                        },
                        "refresh_pixiv_token",
                        serde_json::json!({"credential_id": cred.id}),
                        &state.worker.db,
                        None,
                        None,
                        None,
                        None,
                        Some(&refresh_task_id),
                    )
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

    // --- HTTP server ----------------------------------------------------------
    // Workers run in the separate randimg-worker binary — not here.

    // FastAPI-style startup banner
    let env_name = "production";
    let queue_name = "fang-postgres";

    let worker_mode = if config.color_worker_standalone {
        "standalone (randimg-worker)"
    } else {
        "in-process"
    };
    let routes_list =
        "health, image, auth, tag, statistic, author, crawler, task, pixiv_credential";

    // Mask password in database URLs for logging
    let masked_api_db = mask_database_url(&config.api_database_url);

    tracing::info!(
        "Randimg API Server — v{}\n\
         Environment : {env_name}\n\
         Database    : {}\n\
         Queue       : {queue_name}\n\
         Workers     : {worker_mode}\n\
         Log Level   : {}\n\
         Log Dir     : {}\n\
         Routes      : {routes_list}\n\
         Starting HTTP server…",
        env!("CARGO_PKG_VERSION"),
        masked_api_db,
        config.log_level,
        config.log_dir,
    );

    let cors = if state.worker.config.cors_origins == "*" {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins: Vec<HeaderValue> = state
            .worker
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
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(Arc::new(state.worker.clone()));

    match &config.server_addr {
        BindAddr::Tcp(addr) => {
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            tracing::info!(
                address = %listener.local_addr().unwrap(),
                database = %mask_database_url(&config.api_database_url),
                "Server started (TCP)"
            );
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
                .unwrap();
        }
        BindAddr::Unix(path) => {
            if path.exists() {
                std::fs::remove_file(path).unwrap_or_else(|e| {
                    tracing::warn!("Failed to remove stale socket {:?}: {}", path, e);
                });
            }
            let listener = tokio::net::UnixListener::bind(path).unwrap();
            tracing::info!(
                socket = %path.display(),
                database = %mask_database_url(&config.api_database_url),
                "Server started (Unix socket)"
            );
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
                .unwrap();
        }
    }

    tracing::info!("Shutdown complete");
}

/// Mask password in database URL for safe logging.
/// `postgres://user:secret@host/db` → `postgres://user:***@host/db`
fn mask_database_url(url: &str) -> String {
    if let Some(at_pos) = url.find('@') {
        if let Some(slash_pos) = url[..at_pos].rfind('/') {
            let prefix_end = slash_pos + 2; // skip "://"
            if prefix_end < at_pos {
                let masked = format!("{}{}{}", &url[..prefix_end], "***", &url[at_pos..]);
                return masked;
            }
        }
    }
    url.to_string()
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
