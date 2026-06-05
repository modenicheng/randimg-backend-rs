use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::WorkerState;
use crate::db::query;

pub fn routes() -> Router<Arc<WorkerState>> {
    Router::new().route("/health", get(health_check))
}

pub async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[derive(Serialize)]
pub struct WorkerHealthResponse {
    pub status: &'static str,
    pub uptime_secs: u64,
    pub task_counts: TaskCounts,
    pub active_workers: usize,
}

#[derive(Serialize)]
pub struct TaskCounts {
    pub running: i64,
    pub queued: i64,
    pub failed: i64,
}

pub async fn metrics_handler(
    State(state): State<Arc<WorkerState>>,
) -> Result<Json<query::task::TaskMetrics>, Json<serde_json::Value>> {
    query::task::get_task_metrics(&state.db)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to compute task metrics");
            Json(serde_json::json!({ "error": "failed to compute metrics" }))
        })
}

pub async fn worker_health_handler(
    State(state): State<Arc<WorkerState>>,
) -> Json<WorkerHealthResponse> {
    let uptime = state.worker_start_time.elapsed().as_secs();
    let active = state.active_tasks.load(Ordering::Relaxed);

    let (running, queued, failed) = query::task::count_by_status(&state.db)
        .await
        .unwrap_or((0, 0, 0));

    let watchdog_stuck = crate::watchdog::Watchdog::any_stuck(&state.stuck_pools);

    let status = if watchdog_stuck {
        "unhealthy"
    } else if failed > 10 {
        "unhealthy"
    } else if active == 0 {
        "degraded"
    } else {
        "healthy"
    };

    Json(WorkerHealthResponse {
        status,
        uptime_secs: uptime,
        task_counts: TaskCounts {
            running,
            queued,
            failed,
        },
        active_workers: active,
    })
}
