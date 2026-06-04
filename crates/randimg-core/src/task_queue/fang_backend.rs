//! Fang 异步队列后端
//!
//! 使用 fang 的原生异步 API（`AsyncQueue` + `AsyncQueueable`）管理任务队列。
//! PostgreSQL-only — 通过 `queue-postgres` feature flag 启用。
//!
//! ## 设计要点
//!
//! - `QueueBackend` 封装 `fang::asynk::AsyncQueue`，`Clone` 友好
//! - `push_task()` 在自定义 `tasks` 表和 fang `fang_tasks` 表之间建立关联：
//!   1. `query::task::create()` → 创建自定义任务记录（status: pending）
//!   2. `queue.insert_task()` → 插入 fang 队列（status: new）
//!   3. `query::task::link_fang_task()` → 关联 fang UUID → 自定义任务 ID
//!
//! ## AsyncRunnable 集成说明
//!
//! fang 的 `AsyncRunnable::run(&self, client: &dyn AsyncQueueable)` 接收队列引用，
//! 而非 `WorkerState`。因此 job struct 需要在序列化字段中捕获运行时数据，
//! 并通过全局 `tokio::sync::OnceCell<WorkerState>` 获取共享状态。
//! 具体的 `AsyncRunnable` 实现在各 job 模块中完成（参见 `task_queue/jobs.rs`）。

#[cfg(not(feature = "queue-postgres"))]
compile_error!("Fang async backend requires the 'queue-postgres' feature.");

use crate::config::AppConfig;
use crate::db::query;
use fang::asynk::async_queue::{AsyncQueue, AsyncQueueable};
use fang::asynk::async_runnable::AsyncRunnable;
use sea_orm::DatabaseConnection;
use serde_json::Value as JsonValue;

// ── QueueBackend ─────────────────────────────────────────────

/// Fang 异步队列后端
///
/// 封装 `fang::asynk::AsyncQueue`，通过原生异步 API 操作 fang 任务队列。
/// 内部使用 sqlx 连接池，`Clone` 仅复制连接池引用（无额外连接开销）。
#[derive(Clone, Debug)]
pub struct QueueBackend {
    queue: AsyncQueue,
}

impl QueueBackend {
    /// 从配置创建队列后端并建立连接
    ///
    /// 使用 `config.queue_database_url` 作为 PostgreSQL 连接串。
    /// `max_pool_size` 固定为 10，适用于大多数部署场景。
    ///
    /// # Errors
    ///
    /// 连接失败时返回错误信息（数据库不可达、认证失败等）。
    pub async fn from_config(config: &AppConfig) -> Result<Self, String> {
        let mut queue = AsyncQueue::builder()
            .uri(config.queue_database_url.as_str())
            .max_pool_size(10u32)
            .build();

        queue
            .connect()
            .await
            .map_err(|e| format!("连接 Fang 队列数据库失败: {}", e))?;

        // 初始化 fang 队列表（fang_tasks + fang_task_state enum）
        Self::setup_schema(&config.queue_database_url).await?;

        Ok(Self { queue })
    }

    /// 使用 fang 官方 migration API 初始化队列 schema
    ///
    /// 调用 `fang::run_migrations_postgres()` 创建 `fang_tasks` 表和相关类型。
    /// fang 内部使用 diesel_migrations 追踪已执行的 migration，天然幂等。
    async fn setup_schema(queue_database_url: &str) -> Result<(), String> {
        use diesel::Connection;

        let mut connection = diesel::PgConnection::establish(queue_database_url)
            .map_err(|e| format!("连接 Fang 队列数据库失败（migration）: {}", e))?;

        fang::run_migrations_postgres(&mut connection)
            .map_err(|e| format!("执行 Fang migration 失败: {}", e))?;

        tracing::info!("Fang 队列 migration 完成");
        Ok(())
    }

    /// 推送任务到 fang 队列并同步到自定义任务表
    ///
    /// 流程：
    /// 1. 在 `tasks` 表创建自定义任务记录（状态：pending）
    /// 2. 在 `fang_tasks` 表插入队列任务（状态：new）
    /// 3. 通过 `fang_task_id` 关联两张表，更新 tasks 状态为 queued
    ///
    /// # Arguments
    ///
    /// * `task`      - 实际的 `AsyncRunnable` 实现（如 `CrawlJob`、`DownloadJob`）
    /// * `task_type` - 任务类型标识（如 "crawl"、"download"），写入自定义 tasks 表
    /// * `metadata`  - 任务参数 JSON，写入 `tasks.params`
    /// * `db`        - SeaORM 数据库连接（自定义 tasks 表）
    /// * `parent_id` - 父任务 ID（可选）
    /// * `root_id`   - 根任务 ID（可选）
    /// * `crawler_id`- 关联爬虫 ID（可选）
    /// * `image_id`  - 关联图片 ID（可选）
    ///
    /// # Returns
    ///
    /// 自定义任务的 UUID 字符串 ID。
    ///
    /// # AsyncRunnable 集成
    ///
    /// 此方法通过 `queue.insert_task(task)` 将实际的 AsyncRunnable 插入 fang 队列。
    /// fang 使用 `typetag::serde` 序列化 task struct，worker 反序列化后直接执行
    /// 对应类型的 `AsyncRunnable::run()` 实现。
    pub async fn push_task(
        &self,
        task: &(dyn AsyncRunnable + Send + Sync),
        task_type: &str,
        metadata: JsonValue,
        db: &DatabaseConnection,
        parent_id: Option<&str>,
        root_id: Option<&str>,
        crawler_id: Option<i32>,
        image_id: Option<i32>,
    ) -> Result<String, String> {
        // 1. 创建自定义任务记录
        let task_record = query::task::create(
            db,
            task_type,
            parent_id,
            root_id,
            crawler_id,
            image_id,
            Some(&metadata.to_string()),
        )
        .await
        .map_err(|e| format!("创建任务记录失败: {}", e))?;

        tracing::info!(task_id = %task_record.id, task_type, "Task record created");

        // 2. 插入到 fang 队列 — 使用实际的 AsyncRunnable 实现
        let fang_task = self
            .queue
            .insert_task(task)
            .await
            .map_err(|e| format!("插入 fang 任务失败: {}", e))?;

        let fang_task_id = uuid_to_i64(&fang_task.id);
        tracing::info!(task_id = %task_record.id, fang_task_id, "Pushed to fang queue");

        // 3. 关联 fang 任务 ID（同时更新状态为 queued）
        query::task::link_fang_task(db, &task_record.id, fang_task_id)
            .await
            .map_err(|e| format!("关联 fang 任务失败: {}", e))?;

        tracing::info!(task_id = %task_record.id, "Task queued successfully");
        Ok(task_record.id)
    }

    /// 获取内部 `AsyncQueue` 的不可变引用
    ///
    /// 供 worker 直接操作队列（如 `fetch_and_touch_task`、`schedule_retry`）。
    pub fn queue(&self) -> &AsyncQueue {
        &self.queue
    }
}

// ── 工具函数 ──────────────────────────────────────────────────

/// 将 UUID 转换为 i64（取前 8 字节，大端序）
///
/// 用于将 fang 的 UUID 任务 ID 映射到自定义 tasks 表的 `fang_task_id: i64` 字段。
/// 注意：此转换是有损的，不同 UUID 可能映射到相同的 i64，但在实际使用中碰撞概率极低。
fn uuid_to_i64(uuid: &uuid::Uuid) -> i64 {
    let bytes = uuid.as_bytes();
    i64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}
