//! Single point of backend abstraction for job queues.
//!
//! Re-exports [`QueueBackend`] from the fang async backend and provides a
//! convenience [`init`] function used by `lib.rs` / `main.rs` during startup.

use crate::config::AppConfig;
pub use crate::task_queue::fang_backend::QueueBackend;

/// Initialize the queue backend from application configuration.
///
/// Delegates to [`QueueBackend::from_config`] which connects to the fang
/// PostgreSQL queue and returns a ready-to-use backend.
///
/// # Errors
///
/// Returns an error string if the connection or setup fails.
pub async fn init(config: &AppConfig) -> Result<QueueBackend, String> {
    QueueBackend::from_config(config).await
}
