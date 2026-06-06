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
use crate::db::entities::task_enum::TaskType;
use crate::db::query;
use crate::task_queue::fingerprint::FingerprintCache;
use fang::asynk::async_queue::{AsyncQueue, AsyncQueueable};
use fang::asynk::async_runnable::AsyncRunnable;
use sea_orm::DatabaseConnection;
use sea_orm::TransactionTrait;
use serde_json::Value as JsonValue;
use std::sync::Arc;

// ── QueueBackend ─────────────────────────────────────────────

/// Fang 异步队列后端
///
/// 封装 `fang::asynk::AsyncQueue`，通过原生异步 API 操作 fang 任务队列。
/// 内部使用 sqlx 连接池，`Clone` 仅复制连接池引用（无额外连接开销）。
#[derive(Clone, Debug)]
pub struct QueueBackend {
    queue: AsyncQueue,
    pub fingerprint_cache: Arc<FingerprintCache>,
}

impl QueueBackend {
    /// Create a no-op instance for testing. The internal queue is unconnected —
    /// any operation that pushes/fetches tasks will fail, but the struct can be
    /// held in `WorkerState` for handler-level tests that return early.
    pub fn new_noop() -> Self {
        let queue = AsyncQueue::builder()
            .uri("postgres://localhost/noop")
            .max_pool_size(1u32)
            .build();
        Self {
            queue,
            fingerprint_cache: Arc::new(FingerprintCache::new(300)),
        }
    }

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

        Ok(Self {
            queue,
            fingerprint_cache: Arc::new(FingerprintCache::new(config.task_dedup_ttl_secs)),
        })
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

    /// 推送任务到 fang 队列并同步到自定义任务表（事务包装，保证原子性）
    ///
    /// 流程（在 API 数据库事务中执行，失败时自动回滚，无孤儿任务）：
    /// 1. 开启 API 数据库事务
    /// 2. 在 `tasks` 表创建自定义任务记录（状态：pending）
    /// 3. 在 `fang_tasks` 表插入队列任务（状态：new）— 独立队列数据库
    /// 4. 步骤 3 失败 → 事务回滚，步骤 2 的记录不持久化
    /// 5. 在事务中通过 `fang_task_id` 关联两张表，更新 tasks 状态为 queued
    /// 6. 提交事务
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
        task_id: Option<&str>,
    ) -> Result<String, String> {
        let params_str = metadata.to_string();
        if !self.fingerprint_cache.check_and_insert(task_type, &params_str) {
            tracing::debug!(task_type, "Skipping duplicate task within dedup TTL");
            return Err("duplicate task fingerprint within TTL".into());
        }

        // Parse task_type string to enum for DB storage
        let task_type_enum: TaskType = task_type.parse()
            .map_err(|e: String| format!("Invalid task type '{}': {}", task_type, e))?;

        // 开启事务 — API 数据库上的 SeaORM 事务
        let txn = db
            .begin()
            .await
            .map_err(|e| format!("开启事务失败: {}", e))?;

        // 1. 在事务中创建自定义任务记录
        let task_record = if let Some(tid) = task_id {
            query::task::create_with_id(
                &txn,
                tid,
                task_type_enum.clone(),
                parent_id,
                root_id,
                crawler_id,
                image_id,
                Some(metadata.clone()),
            )
            .await
            .map_err(|e| format!("创建任务记录失败: {}", e))?
        } else {
            query::task::create(
                &txn,
                task_type_enum.clone(),
                parent_id,
                root_id,
                crawler_id,
                image_id,
                Some(metadata.clone()),
            )
            .await
            .map_err(|e| format!("创建任务记录失败: {}", e))?
        };

        tracing::info!(task_id = %task_record.id, task_type, "Task record created in transaction");

        // 2. 插入到 fang 队列 — 独立队列数据库，不在同一个事务中
        //    如果此步骤失败，`txn` 会被 drop → 自动回滚步骤 1 的任务记录
        let fang_task = self
            .queue
            .insert_task(task)
            .await
            .map_err(|e| format!("插入 fang 任务失败: {}", e))?;

        let fang_task_id = fang_task.id.to_string();
        tracing::info!(task_id = %task_record.id, fang_task_id, "Pushed to fang queue");

        // 3. 在事务中关联 fang 任务 ID（同时更新状态为 queued）
        query::task::link_fang_task(&txn, &task_record.id, &fang_task_id)
            .await
            .map_err(|e| format!("关联 fang 任务失败: {}", e))?;

        // 4. 提交事务 — 任务创建 + fang 关联一起持久化
        //    失败时尝试清理孤儿 Fang 任务（best-effort，跨数据库无法保证原子性）
        if let Err(e) = txn.commit().await {
            self.fingerprint_cache.remove(task_type, &params_str);
            let fang_id = fang_task.id;
            if let Err(rm_err) = self.queue.remove_task(&fang_id).await {
                tracing::warn!(fang_task_id = %fang_id, error = %rm_err, "Failed to clean up orphan Fang task after commit failure");
            }
            return Err(format!("提交事务失败: {}", e));
        }

        tracing::info!(task_id = %task_record.id, "Task queued successfully (transaction committed)");
        Ok(task_record.id)
    }

    /// 从 fang 队列中删除指定任务
    ///
    /// 根据 UUID 删除 `fang_tasks` 表中的单条记录。
    /// 注意：此方法仅操作 fang 队列表，不会删除自定义 `tasks` 表中的记录。
    ///
    /// # Arguments
    ///
    /// * `id` - fang 任务的 UUID
    ///
    /// # Returns
    ///
    /// 删除的记录数（0 或 1）。
    pub async fn remove_task(&self, id: &uuid::Uuid) -> Result<u64, String> {
        let removed = self
            .queue
            .remove_task(id)
            .await
            .map_err(|e| format!("删除 fang 任务失败: {}", e))?;

        tracing::debug!(task_id = %id, removed, "Removed fang task");
        Ok(removed)
    }

    /// 从 fang 队列中删除指定类型的所有任务
    ///
    /// 根据 `task_type`（如 "crawl"、"download"）批量删除 `fang_tasks` 表中的记录。
    ///
    /// # Arguments
    ///
    /// * `task_type` - 任务类型标识
    ///
    /// # Returns
    ///
    /// 删除的记录数。
    pub async fn remove_tasks_type(&self, task_type: &str) -> Result<u64, String> {
        let removed = self
            .queue
            .remove_tasks_type(task_type)
            .await
            .map_err(|e| format!("删除 fang 任务类型失败: {}", e))?;

        tracing::debug!(task_type, removed, "Removed fang tasks by type");
        Ok(removed)
    }

    /// 从 fang 队列中删除所有任务
    ///
    /// 清空 `fang_tasks` 表。⚠️ 操作不可逆，请谨慎使用。
    ///
    /// # Returns
    ///
    /// 删除的记录数。
    pub async fn remove_all_tasks(&self) -> Result<u64, String> {
        let removed = self
            .queue
            .remove_all_tasks()
            .await
            .map_err(|e| format!("删除所有 fang 任务失败: {}", e))?;

        tracing::debug!(removed, "Removed all fang tasks");
        Ok(removed)
    }

    /// 获取内部 `AsyncQueue` 的不可变引用
    ///
    /// 供 worker 直接操作队列（如 `fetch_and_touch_task`、`schedule_retry`）。
    pub fn queue(&self) -> &AsyncQueue {
        &self.queue
    }
}


