# Task Status Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix task status inconsistency by adding `task_id` to job structs and calling `update_status()` from `AsyncRunnable::run()` implementations.

**Architecture:** Each job struct gets a `task_id: Option<String>` field. When a job runs, it updates the task status to "running" at start and "done"/"failed" at completion. The `push_task()` method accepts an optional `task_id` parameter to reuse existing task records.

**Tech Stack:** Rust, SeaORM, Fang async queue, tokio

---

## File Structure

**Files to modify:**
- `crates/randimg-core/src/task_queue/jobs.rs` — Add `task_id` field to all 7 job structs, update `run()` methods
- `crates/randimg-core/src/task_queue/fang_backend.rs` — Add `task_id` parameter to `push_task()`
- `crates/randimg-core/src/handlers/crawler.rs` — Update 4 `push_task()` call sites
- `crates/randimg-core/src/handlers/pixiv_credential.rs` — Update 1 `push_task()` call site
- `crates/randimg-core/src/task_queue/handlers.rs` — Update 10 `push_task()` call sites
- `crates/randimg-server/src/main.rs` — Update 1 `push_task()` call site
- `crates/randimg-core/tests/job_test.rs` — Update test structs

---

### Task 1: Add `task_id` field to all job structs

**Files:**
- Modify: `crates/randimg-core/src/task_queue/jobs.rs:36-145`

- [ ] **Step 1: Add `task_id` field to CrawlJob**

```rust
/// Crawl Pixiv illustrations (by user, ranking, or bookmarks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlJob {
    pub crawler_id: i32,
    pub crawl_type: i32,
    pub target_user_id: Option<String>,
    pub target_start_date: Option<String>,
    pub target_end_date: Option<String>,
    pub target_search_prompt: Option<String>,
    /// Ranking mode: "day", "week", "month", "original", "rookie", "daily_r18", "weekly_r18" (default: "day").
    #[serde(default)]
    pub ranking_mode: Option<String>,
    /// User illust type: "illust", "manga" (default: "illust").
    #[serde(default)]
    pub illust_type: Option<String>,
    /// Filter by illust type: list of types to include (e.g., ["illust", "manga"]).
    /// If None or empty, all types are included. Default: None (no filter).
    #[serde(default)]
    pub illust_type_filter: Option<Vec<String>>,
    /// Exclude R18 content (x_restrict > 0). Default: false.
    #[serde(default)]
    pub exclude_r18: Option<bool>,
    /// Exclude AI-generated content (illust_ai_type > 0). Default: false.
    #[serde(default)]
    pub exclude_ai: Option<bool>,
    /// Maximum total pages to crawl (0 or None = unlimited).
    #[serde(default)]
    pub max_pages: Option<u32>,
    /// Max discover hops to run after crawl (override global default).
    #[serde(default)]
    pub discover_hops: Option<u32>,
    /// Max seed limit for discover (override global default).
    #[serde(default)]
    pub discover_seed_limit: Option<u64>,
    /// Seed selection method for discover: "popularity", "views", "bookmarks", "random".
    #[serde(default)]
    pub discover_seed_method: Option<String>,
    /// Disable discover entirely for this crawl job. Default: false.
    #[serde(default)]
    pub disable_discover: Option<bool>,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
}
```

- [ ] **Step 2: Add `task_id` field to DownloadJob**

```rust
/// Download a single image from Pixiv to local disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadJob {
    pub image_id: i32,
    pub source_image_url: String,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// The ID of the root crawl job that originated this pipeline.
    /// Downstream tasks (color_extract, upload, accessibility_check)
    /// use this as their `parent_job_id` so the full pipeline is
    /// represented as direct children of the crawl task.
    #[serde(default)]
    pub root_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
}
```

- [ ] **Step 3: Add `task_id` field to ColorExtractJob**

```rust
/// Extract color palette from a downloaded image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorExtractJob {
    pub image_id: i32,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
}
```

- [ ] **Step 4: Add `task_id` field to UploadJob**

```rust
/// Upload a downloaded image to DogeCloud OSS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadJob {
    pub image_id: i32,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
}
```

- [ ] **Step 5: Add `task_id` field to AccessibilityCheckJob**

```rust
/// Check image accessibility (solid-color detection stub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityCheckJob {
    pub image_id: i32,
    pub image_path: String,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
}
```

- [ ] **Step 6: Add `task_id` field to DiscoverJob**

```rust
/// Discover related illustrations via Pixiv related-illust API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverJob {
    pub hop: u32,
    pub max_hops: Option<u32>,
    pub seed_limit: Option<u64>,
    pub seed_method: Option<String>,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
}
```

- [ ] **Step 7: Add `task_id` field to RefreshPixivTokenJob**

```rust
/// Refresh a Pixiv credential's OAuth token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshPixivTokenJob {
    pub credential_id: i32,
    /// Parent task ID for hierarchy tracking.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Custom task ID for status tracking
    #[serde(default)]
    pub task_id: Option<String>,
}
```

- [ ] **Step 8: Verify compilation**

Run: `cargo check -p randimg-core`
Expected: PASS (fields have `#[serde(default)]` so existing code compiles)

---

### Task 2: Update `AsyncRunnable::run()` implementations with status tracking

**Files:**
- Modify: `crates/randimg-core/src/task_queue/jobs.rs:149-336`

- [ ] **Step 1: Add `use` import for task status constants**

At the top of `jobs.rs`, add:
```rust
use crate::db::entities::task;
use crate::db::query;
```

- [ ] **Step 2: Update CrawlJob::run()**

```rust
#[typetag::serde]
#[async_trait]
impl AsyncRunnable for CrawlJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        let state = worker_state();

        // Update status to running
        if let Some(ref task_id) = self.task_id {
            if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_RUNNING).await {
                tracing::error!(task_id, error = %e, "Failed to update task status to running");
            }
        }

        tracing::info!(crawler_id = self.crawler_id, crawl_type = self.crawl_type, "CrawlJob started");
        let result = handlers::handle_crawl(self.clone(), state).await;

        // Update status based on result
        if let Some(ref task_id) = self.task_id {
            match &result {
                Ok(()) => {
                    if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_DONE).await {
                        tracing::error!(task_id, error = %e, "Failed to update task status to done");
                    }
                }
                Err(e) => {
                    if let Err(update_err) = query::task::update_status(&state.db, task_id, task::STATUS_FAILED).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task status to failed");
                    }
                    if let Err(update_err) = query::task::update_error(&state.db, task_id, &e.to_string()).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task error message");
                    }
                }
            }
        }

        match &result {
            Ok(()) => tracing::info!(crawler_id = self.crawler_id, "CrawlJob completed"),
            Err(e) => tracing::error!(crawler_id = self.crawler_id, error = %e, "CrawlJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    // ... rest unchanged
}
```

- [ ] **Step 3: Update DownloadJob::run()**

```rust
#[typetag::serde]
#[async_trait]
impl AsyncRunnable for DownloadJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        let state = worker_state();

        // Update status to running
        if let Some(ref task_id) = self.task_id {
            if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_RUNNING).await {
                tracing::error!(task_id, error = %e, "Failed to update task status to running");
            }
        }

        tracing::info!(image_id = self.image_id, path = %self.image_path, "DownloadJob started");
        let result = handlers::handle_download(self.clone(), state).await;

        // Update status based on result
        if let Some(ref task_id) = self.task_id {
            match &result {
                Ok(()) => {
                    if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_DONE).await {
                        tracing::error!(task_id, error = %e, "Failed to update task status to done");
                    }
                }
                Err(e) => {
                    if let Err(update_err) = query::task::update_status(&state.db, task_id, task::STATUS_FAILED).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task status to failed");
                    }
                    if let Err(update_err) = query::task::update_error(&state.db, task_id, &e.to_string()).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task error message");
                    }
                }
            }
        }

        match &result {
            Ok(()) => tracing::info!(image_id = self.image_id, "DownloadJob completed"),
            Err(e) => tracing::error!(image_id = self.image_id, error = %e, "DownloadJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    // ... rest unchanged
}
```

- [ ] **Step 4: Update ColorExtractJob::run()**

```rust
#[typetag::serde]
#[async_trait]
impl AsyncRunnable for ColorExtractJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        let state = worker_state();

        // Update status to running
        if let Some(ref task_id) = self.task_id {
            if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_RUNNING).await {
                tracing::error!(task_id, error = %e, "Failed to update task status to running");
            }
        }

        tracing::info!(image_id = self.image_id, path = %self.image_path, "ColorExtractJob started");
        let result = handlers::handle_color_extract(self.clone(), state).await;

        // Update status based on result
        if let Some(ref task_id) = self.task_id {
            match &result {
                Ok(()) => {
                    if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_DONE).await {
                        tracing::error!(task_id, error = %e, "Failed to update task status to done");
                    }
                }
                Err(e) => {
                    if let Err(update_err) = query::task::update_status(&state.db, task_id, task::STATUS_FAILED).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task status to failed");
                    }
                    if let Err(update_err) = query::task::update_error(&state.db, task_id, &e.to_string()).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task error message");
                    }
                }
            }
        }

        match &result {
            Ok(()) => tracing::info!(image_id = self.image_id, "ColorExtractJob completed"),
            Err(e) => tracing::error!(image_id = self.image_id, error = %e, "ColorExtractJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    // ... rest unchanged
}
```

- [ ] **Step 5: Update UploadJob::run()**

```rust
#[typetag::serde]
#[async_trait]
impl AsyncRunnable for UploadJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        let state = worker_state();

        // Update status to running
        if let Some(ref task_id) = self.task_id {
            if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_RUNNING).await {
                tracing::error!(task_id, error = %e, "Failed to update task status to running");
            }
        }

        tracing::info!(image_id = self.image_id, path = %self.image_path, "UploadJob started");
        let result = handlers::handle_upload(self.clone(), state).await;

        // Update status based on result
        if let Some(ref task_id) = self.task_id {
            match &result {
                Ok(()) => {
                    if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_DONE).await {
                        tracing::error!(task_id, error = %e, "Failed to update task status to done");
                    }
                }
                Err(e) => {
                    if let Err(update_err) = query::task::update_status(&state.db, task_id, task::STATUS_FAILED).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task status to failed");
                    }
                    if let Err(update_err) = query::task::update_error(&state.db, task_id, &e.to_string()).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task error message");
                    }
                }
            }
        }

        match &result {
            Ok(()) => tracing::info!(image_id = self.image_id, "UploadJob completed"),
            Err(e) => tracing::error!(image_id = self.image_id, error = %e, "UploadJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    // ... rest unchanged
}
```

- [ ] **Step 6: Update AccessibilityCheckJob::run()**

```rust
#[typetag::serde]
#[async_trait]
impl AsyncRunnable for AccessibilityCheckJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        let state = worker_state();

        // Update status to running
        if let Some(ref task_id) = self.task_id {
            if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_RUNNING).await {
                tracing::error!(task_id, error = %e, "Failed to update task status to running");
            }
        }

        tracing::info!(image_id = self.image_id, path = %self.image_path, "AccessibilityCheckJob started");
        let result = handlers::handle_accessibility_check(self.clone(), state).await;

        // Update status based on result
        if let Some(ref task_id) = self.task_id {
            match &result {
                Ok(()) => {
                    if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_DONE).await {
                        tracing::error!(task_id, error = %e, "Failed to update task status to done");
                    }
                }
                Err(e) => {
                    if let Err(update_err) = query::task::update_status(&state.db, task_id, task::STATUS_FAILED).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task status to failed");
                    }
                    if let Err(update_err) = query::task::update_error(&state.db, task_id, &e.to_string()).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task error message");
                    }
                }
            }
        }

        match &result {
            Ok(()) => tracing::info!(image_id = self.image_id, "AccessibilityCheckJob completed"),
            Err(e) => tracing::error!(image_id = self.image_id, error = %e, "AccessibilityCheckJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    // ... rest unchanged
}
```

- [ ] **Step 7: Update DiscoverJob::run()**

```rust
#[typetag::serde]
#[async_trait]
impl AsyncRunnable for DiscoverJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        let state = worker_state();

        // Update status to running
        if let Some(ref task_id) = self.task_id {
            if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_RUNNING).await {
                tracing::error!(task_id, error = %e, "Failed to update task status to running");
            }
        }

        tracing::info!(hop = self.hop, "DiscoverJob started");
        let result = handlers::handle_discover(self.clone(), state).await;

        // Update status based on result
        if let Some(ref task_id) = self.task_id {
            match &result {
                Ok(()) => {
                    if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_DONE).await {
                        tracing::error!(task_id, error = %e, "Failed to update task status to done");
                    }
                }
                Err(e) => {
                    if let Err(update_err) = query::task::update_status(&state.db, task_id, task::STATUS_FAILED).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task status to failed");
                    }
                    if let Err(update_err) = query::task::update_error(&state.db, task_id, &e.to_string()).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task error message");
                    }
                }
            }
        }

        match &result {
            Ok(()) => tracing::info!(hop = self.hop, "DiscoverJob completed"),
            Err(e) => tracing::error!(hop = self.hop, error = %e, "DiscoverJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    // ... rest unchanged
}
```

- [ ] **Step 8: Update RefreshPixivTokenJob::run()**

```rust
#[typetag::serde]
#[async_trait]
impl AsyncRunnable for RefreshPixivTokenJob {
    async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
        let state = worker_state();

        // Update status to running
        if let Some(ref task_id) = self.task_id {
            if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_RUNNING).await {
                tracing::error!(task_id, error = %e, "Failed to update task status to running");
            }
        }

        tracing::info!(credential_id = self.credential_id, "RefreshPixivTokenJob started");
        let result = handlers::handle_refresh_pixiv_token(self.clone(), state).await;

        // Update status based on result
        if let Some(ref task_id) = self.task_id {
            match &result {
                Ok(()) => {
                    if let Err(e) = query::task::update_status(&state.db, task_id, task::STATUS_DONE).await {
                        tracing::error!(task_id, error = %e, "Failed to update task status to done");
                    }
                }
                Err(e) => {
                    if let Err(update_err) = query::task::update_status(&state.db, task_id, task::STATUS_FAILED).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task status to failed");
                    }
                    if let Err(update_err) = query::task::update_error(&state.db, task_id, &e.to_string()).await {
                        tracing::error!(task_id, error = %update_err, "Failed to update task error message");
                    }
                }
            }
        }

        match &result {
            Ok(()) => tracing::info!(credential_id = self.credential_id, "RefreshPixivTokenJob completed"),
            Err(e) => tracing::error!(credential_id = self.credential_id, error = %e, "RefreshPixivTokenJob failed"),
        }
        result.map_err(|e| FangError { description: e })
    }

    // ... rest unchanged
}
```

- [ ] **Step 9: Verify compilation**

Run: `cargo check -p randimg-core`
Expected: PASS

---

### Task 3: Update `push_task()` signature

**Files:**
- Modify: `crates/randimg-core/src/task_queue/fang_backend.rs:112-155`

- [ ] **Step 1: Update push_task() signature and implementation**

```rust
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
    task_id: Option<&str>,  // NEW PARAMETER
) -> Result<String, String> {
    // 1. 创建自定义任务记录 — 使用提供的 task_id 或生成新的
    let task_record = if let Some(tid) = task_id {
        // 使用提供的 task_id 创建记录
        query::task::create_with_id(
            db,
            tid,
            task_type,
            parent_id,
            root_id,
            crawler_id,
            image_id,
            Some(&metadata.to_string()),
        )
        .await
        .map_err(|e| format!("创建任务记录失败: {}", e))?
    } else {
        // 生成新的 task_id（当前行为）
        query::task::create(
            db,
            task_type,
            parent_id,
            root_id,
            crawler_id,
            image_id,
            Some(&metadata.to_string()),
        )
        .await
        .map_err(|e| format!("创建任务记录失败: {}", e))?
    };

    tracing::info!(task_id = %task_record.id, task_type, "Task record created");

    // 2. 插入到 fang 队列 — 使用实际的 AsyncRunnable 实现
    let fang_task = self
        .queue
        .insert_task(task)
        .await
        .map_err(|e| format!("插入 fang 任务失败: {}", e))?;

    let fang_task_id = fang_task.id.to_string();
    tracing::info!(task_id = %task_record.id, fang_task_id, "Pushed to fang queue");

    // 3. 关联 fang 任务 ID（同时更新状态为 queued）
    query::task::link_fang_task(db, &task_record.id, &fang_task_id)
        .await
        .map_err(|e| format!("关联 fang 任务失败: {}", e))?;

    tracing::info!(task_id = %task_record.id, "Task queued successfully");
    Ok(task_record.id)
}
```

**Note:** This requires adding `create_with_id()` to `query::task` module. See Task 4.

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p randimg-core`
Expected: FAIL (missing `create_with_id` function)

---

### Task 4: Add `create_with_id()` to task query module

**Files:**
- Modify: `crates/randimg-core/src/db/query/task.rs`

- [ ] **Step 1: Add create_with_id() function**

Find the existing `create()` function and add `create_with_id()` after it:

```rust
/// 创建任务记录（使用指定的 ID）
pub async fn create_with_id(
    db: &DatabaseConnection,
    id: &str,
    task_type: &str,
    parent_id: Option<&str>,
    root_id: Option<&str>,
    crawler_id: Option<i32>,
    image_id: Option<i32>,
    params: Option<&str>,
) -> Result<task::Model, DbErr> {
    let now = Utc::now();
    let active = task::ActiveModel {
        id: Set(id.to_string()),
        task_type: Set(task_type.to_string()),
        status: Set(STATUS_PENDING.to_string()),
        parent_id: Set(parent_id.map(|s| s.to_string())),
        root_id: Set(root_id.map(|s| s.to_string())),
        crawler_id: Set(crawler_id),
        image_id: Set(image_id),
        params: Set(params.map(|s| s.to_string())),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };

    active.insert(db).await
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p randimg-core`
Expected: PASS

---

### Task 5: Update call sites in handlers/crawler.rs

**Files:**
- Modify: `crates/randimg-core/src/handlers/crawler.rs:91-131, 179-198, 225-245, 270-289`

- [ ] **Step 1: Add uuid import**

At the top of the file, add:
```rust
use uuid::Uuid;
```

- [ ] **Step 2: Update create_crawler() — CrawlJob**

```rust
    // Submit crawl task to fang job queue
    let task_id = Uuid::new_v4().to_string();
    let crawl_job = CrawlJob {
        crawler_id: crawler.id,
        crawl_type,
        target_user_id: body.target_user_id,
        target_start_date: body.target_start_date.map(|d| d.to_string()),
        target_end_date: body.target_end_date.map(|d| d.to_string()),
        target_search_prompt: body.target_search_prompt,
        ranking_mode: body.ranking_mode,
        illust_type: body.illust_type,
        illust_type_filter: body.illust_type_filter,
        exclude_r18: body.exclude_r18,
        exclude_ai: body.exclude_ai,
        max_pages: body.max_pages,
        discover_hops: body.discover_hops,
        discover_seed_limit: body.discover_seed_limit,
        discover_seed_method: body.discover_seed_method,
        disable_discover: body.disable_discover,
        parent_job_id: None,
        task_id: Some(task_id.clone()),
    };
    state
        .queue_backend
        .push_task(
            &crawl_job,
            "crawl",
            serde_json::to_value(&crawl_job).unwrap_or_default(),
            &state.db,
            None,
            None,
            Some(crawler.id),
            None,
            Some(&task_id),
        )
        .await
        .map_err(|e| AppError::Internal(e))?;
```

- [ ] **Step 3: Update get_crawler_image() — ColorExtractJob**

```rust
        for img in images {
            let task_id = Uuid::new_v4().to_string();
            let color_job = ColorExtractJob {
                image_id: img.id,
                image_path: img.image_path,
                parent_job_id: None,
                task_id: Some(task_id.clone()),
            };
            state
                .queue_backend
                .push_task(
                    &color_job,
                    "color_extract",
                    serde_json::to_value(&color_job).unwrap_or_default(),
                    &state.db,
                    None,
                    None,
                    None,
                    Some(img.id),
                    Some(&task_id),
                )
                .await
                .map_err(|e| AppError::Internal(e))?;
        }
```

- [ ] **Step 4: Update trigger_discover() — DiscoverJob**

```rust
    let task_id = Uuid::new_v4().to_string();
    let discover_job = DiscoverJob {
        hop: 0,
        max_hops: body.max_hops,
        seed_limit: body.seed_limit,
        seed_method: body.seed_method,
        parent_job_id: None,
        task_id: Some(task_id.clone()),
    };
    state
        .queue_backend
        .push_task(
            &discover_job,
            "discover",
            serde_json::to_value(&discover_job).unwrap_or_default(),
            &state.db,
            None,
            None,
            None,
            None,
            Some(&task_id),
        )
        .await
        .map_err(|e| AppError::Internal(e))?;
```

- [ ] **Step 5: Update get_accessibility_queue() — AccessibilityCheckJob**

```rust
        for img in images {
            let task_id = Uuid::new_v4().to_string();
            let a11y_job = AccessibilityCheckJob {
                image_id: img.id,
                image_path: img.image_path,
                parent_job_id: None,
                task_id: Some(task_id.clone()),
            };
            state
                .queue_backend
                .push_task(
                    &a11y_job,
                    "accessibility_check",
                    serde_json::to_value(&a11y_job).unwrap_or_default(),
                    &state.db,
                    None,
                    None,
                    None,
                    Some(img.id),
                    Some(&task_id),
                )
                .await
                .map_err(|e| AppError::Internal(e))?;
        }
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check -p randimg-core`
Expected: PASS

---

### Task 6: Update call sites in handlers/pixiv_credential.rs

**Files:**
- Modify: `crates/randimg-core/src/handlers/pixiv_credential.rs:168-182`

- [ ] **Step 1: Add uuid import**

At the top of the file, add:
```rust
use uuid::Uuid;
```

- [ ] **Step 2: Update refresh_credential() — RefreshPixivTokenJob**

```rust
    let task_id = Uuid::new_v4().to_string();
    let refresh_job = RefreshPixivTokenJob {
        credential_id: id,
        parent_job_id: None,
        task_id: Some(task_id.clone()),
    };
    state
        .queue_backend
        .push_task(
            &refresh_job,
            "refresh_pixiv_token",
            serde_json::to_value(&refresh_job).unwrap_or_default(),
            &state.db,
            None,
            None,
            None,
            None,
            Some(&task_id),
        )
        .await
        .map_err(|e| AppError::Internal(e))?;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p randimg-core`
Expected: PASS

---

### Task 7: Update call sites in task_queue/handlers.rs

**Files:**
- Modify: `crates/randimg-core/src/task_queue/handlers.rs` (10 call sites)

- [ ] **Step 1: Update handle_crawl() — DiscoverJob (line 60-75)**

```rust
            let discover_job = DiscoverJob {
                hop: 0,
                max_hops: job.discover_hops,
                seed_limit: job.discover_seed_limit,
                seed_method: job.discover_seed_method.clone(),
                parent_job_id: Some(current_id.clone()),
                task_id: None,  // No task_id for internal spawns
            };
            let metadata = serde_json::to_value(&discover_job)
                .map_err(|e| format!("Failed to serialize discover job: {}", e))?;
            if let Err(e) = state
                .queue_backend
                .push_task(&discover_job, "discover", metadata, &state.db, Some(&current_id), Some(&current_id), None, None, None)
                .await
            {
                tracing::error!("Failed to submit discover task after crawl: {}", e);
            }
```

- [ ] **Step 2: Update crawl_ranking() — DownloadJob (line 125-138)**

```rust
                let download_job = DownloadJob {
                    image_id: dl.image_id,
                    source_image_url: dl.source_image_url,
                    image_path: dl.image_path,
                    parent_job_id: Some(parent_id.to_string()),
                    root_job_id: Some(parent_id.to_string()),
                    task_id: None,
                };
                let metadata = serde_json::to_value(&download_job)
                    .map_err(|e| format!("Failed to serialize download job: {}", e))?;
                state
                    .queue_backend
                    .push_task(&download_job, "download", metadata, &state.db, Some(parent_id), Some(parent_id), None, Some(dl.image_id), None)
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
```

- [ ] **Step 3: Update crawl_user() — DownloadJob (line 201-214)**

```rust
                let download_job = DownloadJob {
                    image_id: dl.image_id,
                    source_image_url: dl.source_image_url,
                    image_path: dl.image_path,
                    parent_job_id: Some(parent_id.to_string()),
                    root_job_id: Some(parent_id.to_string()),
                    task_id: None,
                };
                let metadata = serde_json::to_value(&download_job)
                    .map_err(|e| format!("Failed to serialize download job: {}", e))?;
                state
                    .queue_backend
                    .push_task(&download_job, "download", metadata, &state.db, Some(parent_id), Some(parent_id), None, Some(dl.image_id), None)
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
```

- [ ] **Step 4: Update crawl_bookmarks() — DownloadJob (line 275-288)**

```rust
                let download_job = DownloadJob {
                    image_id: dl.image_id,
                    source_image_url: dl.source_image_url,
                    image_path: dl.image_path,
                    parent_job_id: Some(parent_id.to_string()),
                    root_job_id: Some(parent_id.to_string()),
                    task_id: None,
                };
                let metadata = serde_json::to_value(&download_job)
                    .map_err(|e| format!("Failed to serialize download job: {}", e))?;
                state
                    .queue_backend
                    .push_task(&download_job, "download", metadata, &state.db, Some(parent_id), Some(parent_id), None, Some(dl.image_id), None)
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
```

- [ ] **Step 5: Update spawn_downstream_children() — ColorExtractJob, UploadJob, AccessibilityCheckJob (line 543-572)**

```rust
    let color_job = ColorExtractJob {
        image_id: job.image_id,
        image_path: job.image_path.clone(),
        parent_job_id: Some(upstream_id.to_string()),
        task_id: None,
    };
    let color_metadata = serde_json::to_value(&color_job).unwrap();

    let upload_job = UploadJob {
        image_id: job.image_id,
        image_path: job.image_path.clone(),
        parent_job_id: Some(upstream_id.to_string()),
        task_id: None,
    };
    let upload_metadata = serde_json::to_value(&upload_job).unwrap();

    let a11y_job = AccessibilityCheckJob {
        image_id: job.image_id,
        image_path: job.image_path.clone(),
        parent_job_id: Some(upstream_id.to_string()),
        task_id: None,
    };
    let a11y_metadata = serde_json::to_value(&a11y_job).unwrap();

    let color_fut = state.queue_backend.push_task(
        &color_job, "color_extract", color_metadata, &state.db, Some(upstream_id), Some(upstream_id), None, Some(job.image_id), None,
    );
    let upload_fut = state.queue_backend.push_task(
        &upload_job, "upload", upload_metadata, &state.db, Some(upstream_id), Some(upstream_id), None, Some(job.image_id), None,
    );
    let a11y_fut = state.queue_backend.push_task(
        &a11y_job, "accessibility_check", a11y_metadata, &state.db, Some(upstream_id), Some(upstream_id), None, Some(job.image_id), None,
    );
```

- [ ] **Step 6: Update handle_discover() — DownloadJob (line 943-956)**

```rust
                let download_job = DownloadJob {
                    image_id: dl.image_id,
                    source_image_url: dl.source_image_url,
                    image_path: dl.image_path,
                    parent_job_id: Some(current_id.clone()),
                    root_job_id: Some(current_id.clone()),
                    task_id: None,
                };
                let metadata = serde_json::to_value(&download_job)
                    .map_err(|e| format!("Failed to serialize download job: {}", e))?;
                state
                    .queue_backend
                    .push_task(&download_job, "download", metadata, &state.db, Some(&current_id), Some(&current_id), None, Some(dl.image_id), None)
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
```

- [ ] **Step 7: Update handle_discover() — DiscoverJob (line 966-979)**

```rust
        let next_discover_job = DiscoverJob {
            hop: hop + 1,
            max_hops: Some(max_hops),
            seed_limit: Some(seed_limit),
            seed_method: job.seed_method.clone(),
            parent_job_id: Some(current_id.clone()),
            task_id: None,
        };
        let metadata = serde_json::to_value(&next_discover_job)
            .map_err(|e| format!("Failed to serialize discover job: {}", e))?;
        state
            .queue_backend
            .push_task(&next_discover_job, "discover", metadata, &state.db, Some(&current_id), Some(&current_id), None, None, None)
            .await
            .map_err(|e| format!("Failed to submit next discover task: {}", e))?;
```

- [ ] **Step 8: Verify compilation**

Run: `cargo check -p randimg-core`
Expected: PASS

---

### Task 8: Update call site in randimg-server/src/main.rs

**Files:**
- Modify: `crates/randimg-server/src/main.rs:143-162`

- [ ] **Step 1: Add uuid import**

At the top of the file, add:
```rust
use uuid::Uuid;
```

- [ ] **Step 2: Update RefreshPixivTokenJob creation**

```rust
                if let Err(e) = state
                    .worker
                    .queue_backend
                    .push_task(
                        &randimg_core::task_queue::jobs::RefreshPixivTokenJob {
                            credential_id: cred.id,
                            parent_job_id: None,
                            task_id: None,
                        },
                        "refresh_pixiv_token",
                        serde_json::json!({"credential_id": cred.id}),
                        &state.worker.db,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .await
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: PASS

---

### Task 9: Update test file

**Files:**
- Modify: `crates/randimg-core/tests/job_test.rs`

- [ ] **Step 1: Update test_crawl_job_roundtrip**

```rust
#[test]
fn test_crawl_job_roundtrip() {
    let job = CrawlJob {
        crawler_id: 1,
        crawl_type: 1,
        target_user_id: Some("12345".into()),
        target_start_date: None,
        target_end_date: None,
        target_search_prompt: Some("landscape".into()),
        ranking_mode: None,
        illust_type: None,
        max_pages: None,
        discover_hops: None,
        discover_seed_limit: None,
        discover_seed_method: None,
        parent_job_id: None,
        exclude_r18: None,
        exclude_ai: None,
        illust_type_filter: None,
        task_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: CrawlJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.crawler_id, 1);
    assert_eq!(deserialized.crawl_type, 1);
    assert_eq!(deserialized.target_user_id.as_deref(), Some("12345"));
    assert_eq!(deserialized.target_search_prompt.as_deref(), Some("landscape"));
    assert!(deserialized.ranking_mode.is_none());
    assert!(deserialized.illust_type.is_none());
    assert!(deserialized.max_pages.is_none());
    assert!(deserialized.discover_hops.is_none());
    assert!(deserialized.discover_seed_limit.is_none());
    assert!(deserialized.discover_seed_method.is_none());
    assert!(deserialized.illust_type_filter.is_none());
    assert!(deserialized.task_id.is_none());
}
```

- [ ] **Step 2: Update test_download_job_roundtrip**

```rust
#[test]
fn test_download_job_roundtrip() {
    let job = DownloadJob {
        image_id: 42,
        source_image_url: "https://example.com/image.jpg".into(),
        image_path: "/data/images/42.jpg".into(),
        parent_job_id: None,
        root_job_id: None,
        task_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: DownloadJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 42);
    assert_eq!(deserialized.source_image_url, "https://example.com/image.jpg");
    assert_eq!(deserialized.image_path, "/data/images/42.jpg");
    assert!(deserialized.task_id.is_none());
}
```

- [ ] **Step 3: Update test_color_extract_job_roundtrip**

```rust
#[test]
fn test_color_extract_job_roundtrip() {
    let job = ColorExtractJob {
        image_id: 10,
        image_path: "/data/images/10.jpg".into(),
        parent_job_id: None,
        task_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: ColorExtractJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 10);
    assert_eq!(deserialized.image_path, "/data/images/10.jpg");
    assert!(deserialized.task_id.is_none());
}
```

- [ ] **Step 4: Update test_upload_job_roundtrip**

```rust
#[test]
fn test_upload_job_roundtrip() {
    let job = UploadJob {
        image_id: 5,
        image_path: "/data/images/5.jpg".into(),
        parent_job_id: None,
        task_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: UploadJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 5);
    assert!(deserialized.task_id.is_none());
}
```

- [ ] **Step 5: Update test_accessibility_check_job_roundtrip**

```rust
#[test]
fn test_accessibility_check_job_roundtrip() {
    let job = AccessibilityCheckJob {
        image_id: 7,
        image_path: "/data/images/7.jpg".into(),
        parent_job_id: None,
        task_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: AccessibilityCheckJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 7);
    assert!(deserialized.task_id.is_none());
}
```

- [ ] **Step 6: Update test_discover_job_roundtrip**

```rust
#[test]
fn test_discover_job_roundtrip() {
    let job = DiscoverJob {
        hop: 0,
        max_hops: Some(3),
        seed_limit: Some(10),
        seed_method: Some("popularity".into()),
        parent_job_id: Some("parent-uuid-123".into()),
        task_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: DiscoverJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.hop, 0);
    assert_eq!(deserialized.max_hops, Some(3));
    assert_eq!(deserialized.seed_limit, Some(10));
    assert_eq!(deserialized.seed_method.as_deref(), Some("popularity"));
    assert!(deserialized.task_id.is_none());
}
```

- [ ] **Step 7: Update test_discover_job_optional_fields_none**

```rust
#[test]
fn test_discover_job_optional_fields_none() {
    let job = DiscoverJob {
        hop: 2,
        max_hops: None,
        seed_limit: None,
        seed_method: None,
        parent_job_id: None,
        task_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: DiscoverJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.hop, 2);
    assert!(deserialized.max_hops.is_none());
    assert!(deserialized.seed_limit.is_none());
    assert!(deserialized.seed_method.is_none());
    assert!(deserialized.task_id.is_none());
}
```

- [ ] **Step 8: Update test_refresh_pixiv_token_job_roundtrip**

```rust
#[test]
fn test_refresh_pixiv_token_job_roundtrip() {
    let job = RefreshPixivTokenJob {
        credential_id: 99,
        parent_job_id: None,
        task_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: RefreshPixivTokenJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.credential_id, 99);
    assert!(deserialized.task_id.is_none());
}
```

- [ ] **Step 9: Update test_crawl_job_deserialize_from_json_literal**

```rust
#[test]
fn test_crawl_job_deserialize_from_json_literal() {
    let json = r#"{
        "crawler_id": 10,
        "crawl_type": 0,
        "target_user_id": null,
        "target_start_date": "2026-01-01",
        "target_end_date": "2026-01-31",
        "target_search_prompt": null,
        "illust_type_filter": null
    }"#;
    let job: CrawlJob = serde_json::from_str(json).unwrap();
    assert_eq!(job.crawler_id, 10);
    assert!(job.target_user_id.is_none());
    assert_eq!(job.target_start_date.as_deref(), Some("2026-01-01"));
    assert!(job.task_id.is_none()); // backward compatible
}
```

- [ ] **Step 10: Update test_job_structs_are_clone**

```rust
#[test]
fn test_job_structs_are_clone() {
    let job = DownloadJob {
        image_id: 1,
        source_image_url: "url".into(),
        image_path: "path".into(),
        parent_job_id: None,
        root_job_id: None,
        task_id: None,
    };
    let cloned = job.clone();
    assert_eq!(cloned.image_id, 1);
}
```

- [ ] **Step 11: Update test_deserialize_without_parent_job_id**

```rust
/// Verify backward compatibility: JSON without parent_job_id or task_id still works
#[test]
fn test_deserialize_without_parent_job_id() {
    let json = r#"{
        "image_id": 42,
        "source_image_url": "https://example.com/img.jpg",
        "image_path": "/data/42.jpg"
    }"#;
    let job: DownloadJob = serde_json::from_str(json).unwrap();
    assert_eq!(job.image_id, 42);
    assert!(job.parent_job_id.is_none());
    assert!(job.task_id.is_none());
}
```

- [ ] **Step 12: Update test_parent_job_id_roundtrip**

```rust
/// Verify parent_job_id and task_id are serialized and deserialized correctly
#[test]
fn test_parent_job_id_roundtrip() {
    let job = CrawlJob {
        crawler_id: 1,
        crawl_type: 0,
        target_user_id: None,
        target_start_date: None,
        target_end_date: None,
        target_search_prompt: None,
        ranking_mode: None,
        illust_type: None,
        max_pages: None,
        discover_hops: None,
        discover_seed_limit: None,
        discover_seed_method: None,
        parent_job_id: Some("parent-uuid-abc".into()),
        exclude_r18: None,
        exclude_ai: None,
        illust_type_filter: None,
        task_id: Some("task-uuid-xyz".into()),
    };
    let json = serde_json::to_string(&job).unwrap();
    assert!(json.contains("parent-uuid-abc"));
    assert!(json.contains("task-uuid-xyz"));
    let deserialized: CrawlJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.parent_job_id.as_deref(), Some("parent-uuid-abc"));
    assert_eq!(deserialized.task_id.as_deref(), Some("task-uuid-xyz"));
}
```

- [ ] **Step 13: Run tests**

Run: `cargo test -p randimg-core --test job_test`
Expected: PASS

---

### Task 10: Final verification

- [ ] **Step 1: Run cargo check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 2: Run tests**

Run: `cargo test -p randimg-core --test job_test`
Expected: PASS

---

## Summary

**Total tasks:** 10
**Total steps:** ~60
**Files modified:** 7
**New functions:** 1 (`create_with_id`)
