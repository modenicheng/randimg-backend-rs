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
use std::sync::Arc;

/// The Apalis connection pool type — determined by feature flag.
pub type ApalisPool = db_backend::Pool;

#[derive(Clone)]
pub struct AppState {
    pub db: sea_orm::DatabaseConnection,
    pub config: AppConfig,
    pub oss: dogecloud::DogeCloudOss,
    pub job_storage: db_backend::JobStorage,
    pub apalis_pool: ApalisPool,
    /// Worker JoinHandles for runtime management (abort/cleanup).
    /// Wrapped in Arc<tokio::sync::Mutex> because JoinHandle is not Clone
    /// and we need shared mutable access from HTTP handlers.
    pub worker_handles: Arc<tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}
