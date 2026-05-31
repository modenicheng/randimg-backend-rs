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

use config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub db: sea_orm::DatabaseConnection,
    pub config: AppConfig,
    pub oss: dogecloud::DogeCloudOss,
    pub job_storage: task_queue::handlers::JobStorage,
    pub apalis_pool: apalis_sqlite::SqlitePool,
}
