use crate::WorkerState;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Worker pool names that the watchdog monitors.
const POOL_NAMES: &[&str] = &[
    "crawl",
    "download",
    "color_extract",
    "upload",
    "accessibility_check",
    "discover",
    "refresh_pixiv_token",
    "cleanup",
];

/// Monitors worker pool health by checking their last-activity timestamps.
///
/// Runs as a background task. Every `check_interval`, it inspects each pool's
/// `last_activity` timestamp in the shared map. Any pool silent for more than
/// `stuck_timeout` is flagged as stuck — a warning is logged and the health
/// status is updated accordingly.
pub struct Watchdog {
    check_interval: Duration,
    stuck_timeout: Duration,
    last_activity: Arc<DashMap<String, Instant>>,
    stuck_pools: Arc<DashMap<String, Instant>>,
}

impl Watchdog {
    pub fn new(
        check_interval: Duration,
        stuck_timeout: Duration,
        last_activity: Arc<DashMap<String, Instant>>,
        stuck_pools: Arc<DashMap<String, Instant>>,
    ) -> Self {
        Self {
            check_interval,
            stuck_timeout,
            last_activity,
            stuck_pools,
        }
    }

    /// Run the watchdog loop. Returns when the shutdown token is cancelled.
    pub async fn run(self, shutdown_token: CancellationToken) {
        tracing::info!(
            check_interval_secs = self.check_interval.as_secs(),
            stuck_timeout_secs = self.stuck_timeout.as_secs(),
            "Watchdog started"
        );

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => {
                    tracing::info!("Watchdog shutting down");
                    break;
                }
                _ = tokio::time::sleep(self.check_interval) => {
                    self.check_all_pools().await;
                }
            }
        }
    }

    async fn check_all_pools(&self) {
        let now = Instant::now();

        for pool_name in POOL_NAMES {
            let pool_key = pool_name.to_string();

            match self.last_activity.get(&pool_key) {
                Some(entry) => {
                    let elapsed = now.duration_since(*entry.value());
                    if elapsed > self.stuck_timeout {
                        self.stuck_pools.insert(pool_key.clone(), now);

                        tracing::warn!(
                            pool = pool_name,
                            elapsed_secs = elapsed.as_secs(),
                            stuck_timeout_secs = self.stuck_timeout.as_secs(),
                            "Worker pool appears stuck — no activity"
                        );
                    } else {
                        // Pool recovered — remove from stuck set
                        self.stuck_pools.remove(&pool_key);
                    }
                }
                None => {
                    // No activity recorded yet — pool hasn't processed any tasks
                    tracing::debug!(pool = pool_name, "Worker pool has no recorded activity yet");
                }
            }
        }
    }

    /// Returns true if any worker pool is currently flagged as stuck.
    pub fn any_stuck(stuck_pools: &DashMap<String, Instant>) -> bool {
        !stuck_pools.is_empty()
    }
}

/// Spawn the watchdog as a background tokio task.
pub fn spawn_watchdog(
    state: Arc<WorkerState>,
    shutdown_token: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let check_interval = Duration::from_secs(state.config.watchdog_check_interval_secs);
    let stuck_timeout = Duration::from_secs(state.config.watchdog_stuck_timeout_secs);

    let watchdog = Watchdog::new(
        check_interval,
        stuck_timeout,
        state.last_activity.clone(),
        state.stuck_pools.clone(),
    );

    tokio::spawn(async move {
        watchdog.run(shutdown_token).await;
    })
}
