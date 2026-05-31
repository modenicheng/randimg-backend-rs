use axum::Router;
use randimg_backend_rs::{config::AppConfig, db, db::query, handlers, task_queue, AppState};
use std::sync::Arc;
use tokio::signal;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_appender::rolling;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};


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

    let oss = randimg_backend_rs::dogecloud::DogeCloudOss::new(
        config.dogecloud_access_key.clone(),
        config.dogecloud_secret_key.clone(),
    );

    let state = Arc::new(AppState {
        db,
        config: config.clone(),
        oss,
    });

    // --- Pixiv credential seed & auto-refresh ----------------------------------
    // Seed from env var if DB has no credentials yet
    if !config.pixiv_refresh_token.is_empty() {
        let existing = query::pixiv_credential::find_all(&state.db).await.unwrap_or_default();
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

    // Submit a refresh task for every active credential
    match query::pixiv_credential::find_all(&state.db).await {
        Ok(creds) => {
            for cred in creds.iter().filter(|c| c.status == 0) {
                if let Err(e) = task_queue::submit_task(
                    &state.db,
                    "refresh_pixiv_token",
                    serde_json::json!({ "credential_id": cred.id }),
                    5,
                )
                .await
                {
                    tracing::error!(
                        cred_id = cred.id,
                        "Failed to submit refresh task: {}", e
                    );
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to list credentials for auto-refresh: {}", e);
        }
    }

    // --- Task runners --------------------------------------------------------
    let runner_handles = task_queue::runner::start_runner(state.clone());

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

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
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.server_addr)
        .await
        .unwrap();
    let local_addr = listener.local_addr().unwrap();

    tracing::info!(
        address = %local_addr,
        database = %config.database_url,
        "Server started"
    );

    // --- Graceful shutdown on CTRL+C =========================================
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    tracing::info!("Shutting down — aborting background task runners…");
    for h in &runner_handles {
        h.abort();
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        for h in runner_handles {
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
