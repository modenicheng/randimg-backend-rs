mod auth;
mod color;
mod config;
mod db;
mod error;
mod handlers;
mod pixiv;
mod task_queue;

use axum::{routing::get, routing::post, Router};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;

use config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub db: sea_orm::DatabaseConnection,
    pub config: AppConfig,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = AppConfig::from_env();

    let db = db::init_database(&config.database_url).await;

    let state = Arc::new(AppState {
        db,
        config: config.clone(),
    });

    // Start background task runner
    task_queue::runner::start_runner(state.clone()).await;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(handlers::image::random_image))
        .route(
            "/image/{image_id}",
            get(handlers::image::get_image)
                .patch(handlers::image::patch_image)
                .delete(handlers::image::delete_image),
        )
        .route("/list", get(handlers::image::list_images))
        .route("/tags", get(handlers::tag::get_tags))
        .route("/statistic", get(handlers::statistic::get_statistic))
        .route("/token", post(handlers::auth::login))
        .route(
            "/crawler",
            get(handlers::crawler::list_crawlers).post(handlers::crawler::create_crawler),
        )
        .route(
            "/crawler/image",
            get(handlers::crawler::get_crawler_image)
                .post(handlers::crawler::error_crawler_image),
        )
        .route("/adjust-accessible", get(handlers::crawler::get_adjust_accessible))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.server_addr)
        .await
        .unwrap();
    tracing::info!("Listening on {}", config.server_addr);
    axum::serve(listener, app).await.unwrap();
}
