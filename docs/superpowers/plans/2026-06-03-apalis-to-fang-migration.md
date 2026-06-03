# Apalis → Fang Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate the task queue system from apalis to fang, with separate database configuration, custom task table, and exposed scheduling config.

**Architecture:** Replace apalis with fang for task queue management. Maintain 3-crate workspace structure (randimg-core, randimg-server, randimg-worker). Use separate databases for API data and queue data. Create custom `tasks` table in API database with sync to fang via API.

**Tech Stack:** Rust 2024, fang 0.11.0-rc1, SeaORM 1, tokio 1, axum 0.8

---

## File Structure

### Files to Create
- `crates/randimg-core/src/task_queue/fang_backend.rs` — Fang queue backend abstraction
- `crates/randimg-core/src/db/entities/task.rs` — Custom task entity for API database
- `crates/randimg-core/src/db/query/task.rs` — Task query functions
- `migration/src/m20260603_001_create_tasks_table.rs` — Migration for tasks table

### Files to Modify
- `Cargo.toml` — Workspace dependencies
- `crates/randimg-core/Cargo.toml` — Replace apalis with fang
- `crates/randimg-core/src/config.rs` — Add API_DATABASE_URL, QUEUE_DATABASE_URL, task scheduling config
- `crates/randimg-core/src/db_backend.rs` — Replace JobStorage with fang-based queue
- `crates/randimg-core/src/task_queue/jobs.rs` — Migrate job structs to fang AsyncRunnable
- `crates/randimg-core/src/task_queue/handlers.rs` — Update handlers for fang
- `crates/randimg-core/src/task_queue/mod.rs` — Update module exports
- `crates/randimg-core/src/handlers/task.rs` — Update to use new tasks table
- `crates/randimg-core/src/db/entities/mod.rs` — Add task module
- `crates/randimg-core/src/db/query/mod.rs` — Add task module
- `crates/randimg-core/src/db/query/apalis_job.rs` — Remove or deprecate
- `crates/randimg-core/src/db/query/task_tree.rs` — Update to use new tasks table
- `crates/randimg-core/src/db/query/task_dependency.rs` — Update to use new tasks table
- `crates/randimg-core/src/lib.rs` — Update WorkerState, remove apalis_pool
- `crates/randimg-server/src/main.rs` — Update server initialization
- `crates/randimg-worker/src/main.rs` — Update worker initialization
- `crates/randimg-core/src/error.rs` — Add fang error conversion

---

## Task 1: Add Fang Dependency, Remove Apalis

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/randimg-core/Cargo.toml`

- [ ] **Step 1: Update workspace Cargo.toml**

```toml
# Cargo.toml (workspace root)
[workspace.dependencies]
# Remove apalis
# apalis = { version = "1.0.0-rc.9", features = ["extensions", "retry"] }
# apalis-codec = "0.6.3"

# Add fang
fang = "0.11.0-rc1"

# Keep existing dependencies
sea-orm = { version = "1", features = [...] }
tokio = { version = "1", features = ["full"] }
# ... etc
```

- [ ] **Step 2: Update randimg-core Cargo.toml**

```toml
# crates/randimg-core/Cargo.toml
[dependencies]
# Remove apalis dependencies
# apalis = { workspace = true }
# apalis-codec = { workspace = true }
# apalis-sqlite = { workspace = true, optional = true }
# apalis-postgres = { workspace = true, optional = true }
# apalis-redis = { workspace = true, optional = true }

# Add fang
fang = { workspace = true }

# Remove sqlx (no longer needed for direct apalis queries)
# sqlx = { workspace = true, features = [...], optional = true }

[features]
default = ["db-sqlite", "queue-sqlite"]
db-sqlite = ["sea-orm/sqlx-sqlite"]
db-postgres = ["sea-orm/sqlx-postgres"]
queue-sqlite = ["fang/sqlite"]
queue-postgres = ["fang/postgres"]
# Remove queue-redis (no longer needed)
# queue-redis = ["apalis-redis"]
```

- [ ] **Step 3: Run cargo check**

Run: `cargo check`
Expected: Compilation errors (expected - we'll fix in subsequent tasks)

---

## Task 2: Update Config for Separate Databases

**Files:**
- Modify: `crates/randimg-core/src/config.rs`

- [ ] **Step 1: Add new config fields**

```rust
// crates/randimg-core/src/config.rs

/// 应用配置
/// 
/// 设计思路：
/// - 分离 API 数据库和队列数据库，便于独立维护和扩展
/// - 暴露任务调度配置，允许运维人员根据负载调整
/// - 保持向后兼容，旧的 DATABASE_URL 作为 fallback
pub struct AppConfig {
    // API 数据库 (SeaORM) - 存储业务数据和自定义任务表
    pub api_database_url: String,
    
    // 队列数据库 (Fang) - 存储 fang_tasks 表
    pub queue_database_url: String,
    
    // 任务调度配置
    pub task_max_retries: i32,           // 最大重试次数
    pub task_backoff_base: u32,          // 退避基数（指数退避）
    pub task_poll_interval_ms: u64,      // 轮询间隔（毫秒）
    pub task_default_timeout_secs: u64,  // 默认超时时间
    
    // 各任务类型的并发数
    pub task_concurrency_crawl: u32,
    pub task_concurrency_download: u32,
    pub task_concurrency_color_extract: u32,
    pub task_concurrency_upload: u32,
    pub task_concurrency_accessibility_check: u32,
    pub task_concurrency_discover: u32,
    pub task_concurrency_refresh_pixiv_token: u32,
    
    // ... existing fields ...
}

impl AppConfig {
    pub fn from_env() -> Result<Self, Error> {
        // 兼容旧配置：如果没有新配置项，使用 DATABASE_URL
        let api_database_url = std::env::var("API_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| "sqlite://data/randimg.db?mode=rwc".to_string());
        
        let queue_database_url = std::env::var("QUEUE_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| "sqlite://data/fang.db?mode=rwc".to_string());
        
        // 解析任务调度配置
        let task_max_retries = std::env::var("TASK_MAX_RETRIES")
            .unwrap_or_else(|_| "3".to_string())
            .parse()?;
        
        let task_backoff_base = std::env::var("TASK_BACKOFF_BASE")
            .unwrap_or_else(|_| "2".to_string())
            .parse()?;
        
        let task_poll_interval_ms = std::env::var("TASK_POLL_INTERVAL_MS")
            .unwrap_or_else(|_| "500".to_string())
            .parse()?;
        
        // ... 解析其他配置 ...
        
        Ok(Self {
            api_database_url,
            queue_database_url,
            task_max_retries,
            task_backoff_base,
            task_poll_interval_ms,
            // ... other fields ...
        })
    }
}
```

- [ ] **Step 2: Update .env.example**

```bash
# .env.example

# API 数据库 (SeaORM)
API_DATABASE_URL=sqlite://data/randimg.db?mode=rwc

# 队列数据库 (Fang)
QUEUE_DATABASE_URL=sqlite://data/fang.db?mode=rwc

# 任务调度配置
TASK_MAX_RETRIES=3
TASK_BACKOFF_BASE=2
TASK_POLL_INTERVAL_MS=500
TASK_DEFAULT_TIMEOUT_SECS=300

# 各任务类型并发数
TASK_CONCURRENCY_CRAWL=2
TASK_CONCURRENCY_DOWNLOAD=4
TASK_CONCURRENCY_COLOR_EXTRACT=2
TASK_CONCURRENCY_UPLOAD=2
TASK_CONCURRENCY_ACCESSIBILITY_CHECK=2
TASK_CONCURRENCY_DISCOVER=1
TASK_CONCURRENCY_REFRESH_PIXIV_TOKEN=1
```

---

## Task 3: Create Custom Tasks Table Entity

**Files:**
- Create: `crates/randimg-core/src/db/entities/task.rs`
- Modify: `crates/randimg-core/src/db/entities/mod.rs`

- [ ] **Step 1: Create task entity**

```rust
// crates/randimg-core/src/db/entities/task.rs

/// 自定义任务表实体
/// 
/// 设计思路：
/// - 与 fang_tasks 表分离，避免直接依赖 fang 内部实现
/// - 存储业务相关的元数据（parent_id, root_id, crawler_id 等）
/// - 通过 fang_task_id 关联 fang 任务，便于状态同步
/// - 支持自定义状态流转，不受 fang 状态限制
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// 任务状态常量
/// 
/// 与 Apalis 状态保持兼容，便于平滑迁移
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_KILLED: &str = "killed";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tasks")]
pub struct Model {
    /// 主键，使用 UUID 或 ULID
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    
    /// Fang 任务 ID，用于关联 fang_tasks 表
    pub fang_task_id: Option<i64>,
    
    /// 任务类型：crawl, download, color_extract, upload, accessibility_check, discover, refresh_pixiv_token
    pub task_type: String,
    
    /// 任务状态：pending, queued, running, done, failed, killed
    pub status: String,
    
    /// 父任务 ID，用于任务树结构
    pub parent_id: Option<String>,
    
    /// 根任务 ID，便于快速查询任务树
    pub root_id: Option<String>,
    
    /// 爬虫 ID（crawl 任务专用）
    pub crawler_id: Option<i32>,
    
    /// 图片 ID（download/upload 任务专用）
    pub image_id: Option<i32>,
    
    /// 任务参数（JSON 格式）
    pub params: Option<String>,
    
    /// 错误信息
    pub error_message: Option<String>,
    
    /// 重试次数
    pub retry_count: i32,
    
    /// 创建时间
    pub created_at: DateTimeWithTimeZone,
    
    /// 更新时间
    pub updated_at: DateTimeWithTimeZone,
    
    /// 完成时间
    pub completed_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

- [ ] **Step 2: Update entities mod.rs**

```rust
// crates/randimg-core/src/db/entities/mod.rs

pub mod apalis_job;  // 保留用于兼容，后续移除
pub mod task;        // 新增自定义任务表

// ... existing modules ...
```

---

## Task 4: Create Task Query Functions

**Files:**
- Create: `crates/randimg-core/src/db/query/task.rs`
- Modify: `crates/randimg-core/src/db/query/mod.rs`

- [ ] **Step 1: Create task query functions**

```rust
// crates/randimg-core/src/db/query/task.rs

/// 任务查询函数
/// 
/// 设计思路：
/// - 提供与 apalis_job 查询相同的功能接口
/// - 使用 SeaORM 查询构建器，支持 SQLite 和 PostgreSQL
/// - 包含任务树查询功能（递归 CTE）
use sea_orm::*;
use crate::db::entities::task::{self, Entity, Model, STATUS_PENDING, STATUS_QUEUED, STATUS_RUNNING, STATUS_DONE, STATUS_FAILED, STATUS_KILLED};

/// 列出任务（分页）
pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    status: Option<&str>,
    task_type: Option<&str>,
) -> Result<(Vec<Model>, u64), DbErr> {
    let mut query = Entity::find();
    
    if let Some(status) = status {
        query = query.filter(task::Column::Status.eq(status));
    }
    
    if let Some(task_type) = task_type {
        query = query.filter(task::Column::TaskType.eq(task_type));
    }
    
    let total = query.clone().count(db).await?;
    
    let tasks = query
        .order_by_desc(task::Column::CreatedAt)
        .paginate(db, page_size)
        .fetch_page(page)
        .await?;
    
    Ok((tasks, total))
}

/// 根据 ID 查找任务
pub async fn find_by_id(db: &DatabaseConnection, id: &str) -> Result<Option<Model>, DbErr> {
    Entity::find_by_id(id).one(db).await
}

/// 删除任务
pub async fn delete_by_id(db: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
    let result = Entity::delete_by_id(id).exec(db).await?;
    Ok(result.rows_affected > 0)
}

/// 删除待处理任务
pub async fn delete_pending(db: &DatabaseConnection) -> Result<u64, DbErr> {
    let result = Entity::delete_many()
        .filter(task::Column::Status.eq(STATUS_PENDING))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

/// 按状态删除任务
pub async fn delete_by_statuses(db: &DatabaseConnection, statuses: &[&str]) -> Result<u64, DbErr> {
    let result = Entity::delete_many()
        .filter(task::Column::Status.is_in(statuses.to_vec()))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

/// 按类型查找爬虫 ID
pub async fn find_crawl_ids_by_type(db: &DatabaseConnection, task_type: &str) -> Result<Vec<i32>, DbErr> {
    let tasks = Entity::find()
        .filter(task::Column::TaskType.eq(task_type))
        .filter(task::Column::CrawlerId.is_not_null())
        .all(db)
        .await?;
    
    Ok(tasks.into_iter().filter_map(|t| t.crawler_id).collect())
}

/// 更新任务状态
pub async fn update_status(db: &DatabaseConnection, id: &str, status: &str) -> Result<bool, DbErr> {
    let task = Entity::find_by_id(id).one(db).await?;
    
    if let Some(task) = task {
        let mut active_model: task::ActiveModel = task.into();
        active_model.status = Set(status.to_string());
        active_model.updated_at = Set(chrono::Utc::now().into());
        
        if status == STATUS_DONE || status == STATUS_FAILED || status == STATUS_KILLED {
            active_model.completed_at = Set(Some(chrono::Utc::now().into()));
        }
        
        active_model.update(db).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// 更新任务错误信息
pub async fn update_error(db: &DatabaseConnection, id: &str, error: &str) -> Result<bool, DbErr> {
    let task = Entity::find_by_id(id).one(db).await?;
    
    if let Some(task) = task {
        let mut active_model: task::ActiveModel = task.into();
        active_model.error_message = Set(Some(error.to_string()));
        active_model.updated_at = Set(chrono::Utc::now().into());
        
        active_model.update(db).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// 增加重试次数
pub async fn increment_retry(db: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
    let task = Entity::find_by_id(id).one(db).await?;
    
    if let Some(task) = task {
        let mut active_model: task::ActiveModel = task.into();
        active_model.retry_count = Set(task.retry_count + 1);
        active_model.updated_at = Set(chrono::Utc::now().into());
        
        active_model.update(db).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// 创建新任务
pub async fn create(
    db: &DatabaseConnection,
    id: &str,
    task_type: &str,
    parent_id: Option<&str>,
    root_id: Option<&str>,
    crawler_id: Option<i32>,
    image_id: Option<i32>,
    params: Option<&str>,
) -> Result<Model, DbErr> {
    let now = chrono::Utc::now().into();
    
    let active_model = task::ActiveModel {
        id: Set(id.to_string()),
        fang_task_id: Set(None),
        task_type: Set(task_type.to_string()),
        status: Set(STATUS_PENDING.to_string()),
        parent_id: Set(parent_id.map(|s| s.to_string())),
        root_id: Set(root_id.map(|s| s.to_string())),
        crawler_id: Set(crawler_id),
        image_id: Set(image_id),
        params: Set(params.map(|s| s.to_string())),
        error_message: Set(None),
        retry_count: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
        completed_at: Set(None),
    };
    
    active_model.insert(db).await
}

/// 链接 Fang 任务 ID
pub async fn link_fang_task(db: &DatabaseConnection, id: &str, fang_task_id: i64) -> Result<bool, DbErr> {
    let task = Entity::find_by_id(id).one(db).await?;
    
    if let Some(task) = task {
        let mut active_model: task::ActiveModel = task.into();
        active_model.fang_task_id = Set(Some(fang_task_id));
        active_model.status = Set(STATUS_QUEUED.to_string());
        active_model.updated_at = Set(chrono::Utc::now().into());
        
        active_model.update(db).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}
```

- [ ] **Step 2: Update query mod.rs**

```rust
// crates/randimg-core/src/db/query/mod.rs

pub mod apalis_job;  // 保留用于兼容，后续移除
pub mod task;        // 新增自定义任务查询
pub mod task_tree;
pub mod task_dependency;

// ... existing modules ...
```

---

## Task 4.5: Task Tree Design with root_id

**Files:**
- Modify: `crates/randimg-core/src/db/query/task.rs`

**设计思路：**

使用 `root_id` 和 `parent_id` 实现扁平化的任务树查询，避免递归 CTE 的复杂性。

**任务树结构：**
```
root_task (id=1, root_id=1, parent_id=null)
├── child_task_1 (id=2, root_id=1, parent_id=1)
│   ├── grandchild_1 (id=4, root_id=1, parent_id=2)
│   └── grandchild_2 (id=5, root_id=1, parent_id=2)
└── child_task_2 (id=3, root_id=1, parent_id=1)
```

**关键设计：**
1. **root_id**：根任务的 ID，所有子任务都继承根任务的 ID
2. **parent_id**：直接父任务的 ID，用于构建树结构
3. **扁平查询**：通过 `WHERE root_id = ?` 直接获取整个任务树，无需递归
4. **派生状态**：根任务的状态由子任务状态计算得出

- [ ] **Step 1: Add task tree query functions**

```rust
// crates/randimg-core/src/db/query/task.rs

/// 获取整个任务树（扁平列表）
/// 
/// 设计思路：
/// - 使用 root_id 直接查询，避免递归 CTE
/// - 返回扁平列表，前端可自行构建树结构
/// - 按创建时间排序，便于观察任务执行顺序
pub async fn get_task_tree(
    db: &DatabaseConnection,
    root_id: &str,
) -> Result<Vec<Model>, DbErr> {
    Entity::find()
        .filter(task::Column::RootId.eq(root_id))
        .order_by_asc(task::Column::CreatedAt)
        .all(db)
        .await
}

/// 获取根任务列表（分页）
/// 
/// 设计思路：
/// - 根任务是 parent_id 为 null 的任务
/// - 支持按状态和类型过滤
/// - 返回派生状态（基于子任务计算）
pub async fn list_roots(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    status: Option<&str>,
    task_type: Option<&str>,
) -> Result<(Vec<RootTaskWithStatus>, u64), DbErr> {
    // 1. 查询根任务
    let mut query = Entity::find()
        .filter(task::Column::ParentId.is_null());
    
    if let Some(task_type) = task_type {
        query = query.filter(task::Column::TaskType.eq(task_type));
    }
    
    let total = query.clone().count(db).await?;
    
    let roots = query
        .order_by_desc(task::Column::CreatedAt)
        .paginate(db, page_size)
        .fetch_page(page)
        .await?;
    
    // 2. 为每个根任务计算派生状态
    let mut result = Vec::new();
    for root in roots {
        let tree = get_task_tree(db, &root.id).await?;
        let derived_status = compute_derived_status(&tree);
        
        // 如果指定了状态过滤，跳过不匹配的
        if let Some(filter_status) = status {
            if derived_status != filter_status {
                continue;
            }
        }
        
        result.push(RootTaskWithStatus {
            task: root,
            derived_status,
            total_subtasks: tree.len() - 1,  // 不包含根任务自身
            completed_subtasks: tree.iter()
                .filter(|t| t.status == STATUS_DONE)
                .count() - 1,
        });
    }
    
    Ok((result, total))
}

/// 计算派生状态
/// 
/// 设计思路：
/// - 根任务的状态由子任务状态决定
/// - 规则：
///   - 所有子任务完成 → 根任务完成
///   - 任一子任务失败 → 根任务失败
///   - 任一子任务运行中 → 根任务运行中
///   - 所有子任务待处理 → 根任务待处理
fn compute_derived_status(tasks: &[Model]) -> String {
    if tasks.is_empty() {
        return STATUS_PENDING.to_string();
    }
    
    let has_failed = tasks.iter().any(|t| t.status == STATUS_FAILED);
    if has_failed {
        return STATUS_FAILED.to_string();
    }
    
    let has_running = tasks.iter().any(|t| t.status == STATUS_RUNNING);
    if has_running {
        return STATUS_RUNNING.to_string();
    }
    
    let all_done = tasks.iter().all(|t| t.status == STATUS_DONE);
    if all_done {
        return STATUS_DONE.to_string();
    }
    
    let has_queued = tasks.iter().any(|t| t.status == STATUS_QUEUED);
    if has_queued {
        return STATUS_QUEUED.to_string();
    }
    
    STATUS_PENDING.to_string()
}

/// 根任务及其派生状态
#[derive(Debug, Serialize, Deserialize)]
pub struct RootTaskWithStatus {
    pub task: Model,
    pub derived_status: String,
    pub total_subtasks: usize,
    pub completed_subtasks: usize,
}

/// 创建子任务时设置 root_id
/// 
/// 设计思路：
/// - 如果有父任务，继承父任务的 root_id
/// - 如果没有父任务，自己的 id 就是 root_id
pub async fn create_with_parent(
    db: &DatabaseConnection,
    id: &str,
    task_type: &str,
    parent_id: Option<&str>,
    crawler_id: Option<i32>,
    image_id: Option<i32>,
    params: Option<&str>,
) -> Result<Model, DbErr> {
    // 确定 root_id
    let root_id = if let Some(parent_id) = parent_id {
        // 查找父任务，继承其 root_id
        let parent = Entity::find_by_id(parent_id).one(db).await?;
        parent
            .map(|p| p.root_id.unwrap_or_else(|| p.id.clone()))
            .unwrap_or_else(|| parent_id.to_string())
    } else {
        // 没有父任务，自己就是根
        id.to_string()
    };
    
    create(db, id, task_type, parent_id, Some(&root_id), crawler_id, image_id, params).await
}
```

- [ ] **Step 2: Add indexes for efficient tree queries**

```rust
// migration/src/m20260603_001_create_tasks_table.rs

// 在创建表后添加索引
manager
    .create_index(
        Index::create()
            .table(Tasks::Table)
            .col(Tasks::RootId)
            .to_owned(),
    )
    .await?;

manager
    .create_index(
        Index::create()
            .table(Tasks::Table)
            .col(Tasks::ParentId)
            .to_owned(),
    )
    .await?;

// 复合索引：根任务查询
manager
    .create_index(
        Index::create()
            .table(Tasks::Table)
            .col(Tasks::ParentId)
            .col(Tasks::CreatedAt)
            .to_owned(),
    )
    .await?;
```

**API 响应示例：**

```json
{
  "task": {
    "id": "1",
    "task_type": "crawl",
    "status": "running",
    "parent_id": null,
    "root_id": "1",
    "crawler_id": 42,
    "created_at": "2026-06-03T10:00:00Z"
  },
  "derived_status": "running",
  "total_subtasks": 5,
  "completed_subtasks": 2,
  "subtasks": [
    {
      "id": "2",
      "task_type": "download",
      "status": "done",
      "parent_id": "1",
      "root_id": "1",
      "image_id": 100
    },
    {
      "id": "3",
      "task_type": "download",
      "status": "running",
      "parent_id": "1",
      "root_id": "1",
      "image_id": 101
    }
  ]
}
```

---

## Task 5: Create Fang Queue Backend

**Files:**
- Create: `crates/randimg-core/src/task_queue/fang_backend.rs`

- [ ] **Step 1: Create fang backend abstraction**

```rust
// crates/randimg-core/src/task_queue/fang_backend.rs

/// Fang 队列后端抽象
/// 
/// 设计思路：
/// - 封装 fang 的 AsyncQueueable 接口，提供统一的任务推送 API
/// - 支持 SQLite 和 PostgreSQL 两种后端
/// - 与自定义 tasks 表同步，维护任务状态一致性
/// - 使用配置文件中的参数控制重试策略
use std::sync::Arc;
use fang::{AsyncQueueable, AsyncRunnable, FangError, PostgresPool, SqlitePool};
use crate::config::AppConfig;
use crate::db::query::task as task_query;
use sea_orm::DatabaseConnection;

/// 队列后端枚举
/// 
/// 支持 SQLite 和 PostgreSQL，通过 feature flags 选择
pub enum QueueBackend {
    Sqlite(SqlitePool),
    Postgres(PostgresPool),
}

impl QueueBackend {
    /// 从配置创建队列后端
    pub async fn from_config(config: &AppConfig) -> Result<Self, FangError> {
        #[cfg(feature = "queue-sqlite")]
        {
            let pool = SqlitePool::new(&config.queue_database_url).await?;
            Ok(QueueBackend::Sqlite(pool))
        }
        
        #[cfg(feature = "queue-postgres")]
        {
            let pool = PostgresPool::new(&config.queue_database_url).await?;
            Ok(QueueBackend::Postgres(pool))
        }
    }
    
    /// 推送任务到队列
    /// 
    /// 同时更新自定义 tasks 表和 fang 队列
    pub async fn push_task(
        &self,
        task: &dyn AsyncRunnable,
        api_task_id: &str,
        db: &DatabaseConnection,
    ) -> Result<i64, FangError> {
        // 1. 推送到 fang 队列
        let fang_task_id = match self {
            QueueBackend::Sqlite(pool) => {
                pool.insert_task(task).await?
            }
            QueueBackend::Postgres(pool) => {
                pool.insert_task(task).await?
            }
        };
        
        // 2. 更新自定义 tasks 表
        task_query::link_fang_task(db, api_task_id, fang_task_id)
            .await
            .map_err(|e| FangError::from(e.to_string()))?;
        
        Ok(fang_task_id)
    }
    
    /// 获取队列引用（用于 worker）
    pub fn as_queueable(&self) -> &dyn AsyncQueueable {
        match self {
            QueueBackend::Sqlite(pool) => pool,
            QueueBackend::Postgres(pool) => pool,
        }
    }
}
```

---

## Task 6: Migrate Job Definitions to Fang

**Files:**
- Modify: `crates/randimg-core/src/task_queue/jobs.rs`

- [ ] **Step 1: Update job structs for fang**

```rust
// crates/randimg-core/src/task_queue/jobs.rs

/// 任务定义
/// 
/// 设计思路：
/// - 实现 fang 的 AsyncRunnable trait 替代 apalis 的 Job trait
/// - 使用 task_type() 方法区分不同任务类型
/// - 从配置文件读取重试策略
/// - 保留 parent_job_id 用于任务树追踪
use fang::{AsyncRunnable, FangError, typed::serde_json};
use serde::{Deserialize, Serialize};

/// CrawlJob - 爬虫任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlJob {
    pub crawler_id: i32,
    pub parent_job_id: Option<String>,
}

#[typetag::serde]
impl AsyncRunnable for CrawlJob {
    /// 任务类型标识
    fn task_type(&self) -> String {
        "crawl".to_string()
    }
    
    /// 最大重试次数
    fn max_retries(&self) -> i32 {
        // 从配置读取，运行时动态获取
        crate::config::get_config()
            .map(|c| c.task_max_retries)
            .unwrap_or(3)
    }
    
    /// 退避策略（指数退避）
    fn backoff(&self, attempt: u32) -> u32 {
        let base = crate::config::get_config()
            .map(|c| c.task_backoff_base)
            .unwrap_or(2);
        u32::pow(base, attempt)
    }
    
    /// 执行任务
    async fn run(&self, queueable: &mut dyn fang::AsyncQueueable) -> Result<(), FangError> {
        // 任务逻辑保持不变
        // 需要从 WorkerState 获取依赖
        Ok(())
    }
}

/// DownloadJob - 下载任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadJob {
    pub image_id: i32,
    pub parent_job_id: Option<String>,
}

#[typetag::serde]
impl AsyncRunnable for DownloadJob {
    fn task_type(&self) -> String {
        "download".to_string()
    }
    
    fn max_retries(&self) -> i32 {
        crate::config::get_config()
            .map(|c| c.task_max_retries)
            .unwrap_or(3)
    }
    
    fn backoff(&self, attempt: u32) -> u32 {
        let base = crate::config::get_config()
            .map(|c| c.task_backoff_base)
            .unwrap_or(2);
        u32::pow(base, attempt)
    }
    
    async fn run(&self, queueable: &mut dyn fang::AsyncQueueable) -> Result<(), FangError> {
        Ok(())
    }
}

// ... 类似定义其他任务类型 ...
```

---

## Task 7: Update Task Handlers for Fang

**Files:**
- Modify: `crates/randimg-core/src/task_queue/handlers.rs`

- [ ] **Step 1: Update handlers to use fang**

```rust
// crates/randimg-core/src/task_queue/handlers.rs

/// 任务处理器
/// 
/// 设计思路：
/// - 使用 fang 的 AsyncRunnable::run() 方法执行任务
/// - 从 WorkerState 获取依赖（数据库、配置、HTTP 客户端等）
/// - 更新自定义 tasks 表的状态
/// - 保持原有业务逻辑不变
use std::sync::Arc;
use fang::{AsyncQueueable, FangError};
use crate::WorkerState;
use crate::db::query::task as task_query;

/// CrawlJob 处理器
pub async fn handle_crawl_job(
    job: &CrawlJob,
    state: &Arc<WorkerState>,
    queueable: &mut dyn AsyncQueueable,
) -> Result<(), FangError> {
    // 更新状态为 running
    if let Some(task_id) = &job.parent_job_id {
        task_query::update_status(&state.db, task_id, "running")
            .await
            .map_err(|e| FangError::from(e.to_string()))?;
    }
    
    // 执行原有业务逻辑
    let result = execute_crawl_logic(job, state).await;
    
    // 更新状态
    if let Some(task_id) = &job.parent_job_id {
        match &result {
            Ok(_) => {
                task_query::update_status(&state.db, task_id, "done")
                    .await
                    .map_err(|e| FangError::from(e.to_string()))?;
            }
            Err(e) => {
                task_query::update_status(&state.db, task_id, "failed")
                    .await
                    .map_err(|e| FangError::from(e.to_string()))?;
                task_query::update_error(&state.db, task_id, &e.to_string())
                    .await
                    .map_err(|e| FangError::from(e.to_string()))?;
            }
        }
    }
    
    result
}

// ... 类似更新其他处理器 ...
```

---

## Task 8: Update db_backend.rs

**Files:**
- Modify: `crates/randimg-core/src/db_backend.rs`

- [ ] **Step 1: Replace JobStorage with fang backend**

```rust
// crates/randimg-core/src/db_backend.rs

/// 数据库后端抽象
/// 
/// 设计思路：
/// - 移除 apalis 的 JobStorage 和 Pool 类型
/// - 使用 fang::QueueBackend 替代
/// - 保持向后兼容的接口
use std::sync::Arc;
use sea_orm::DatabaseConnection;
use crate::config::AppConfig;
use crate::task_queue::fang_backend::QueueBackend;

/// 初始化数据库连接
/// 
/// 分别初始化 API 数据库和队列数据库
pub async fn init(config: &AppConfig) -> Result<(DatabaseConnection, QueueBackend), Error> {
    // 初始化 API 数据库
    let api_db = sea_orm::Database::connect(&config.api_database_url).await?;
    
    // 初始化队列数据库
    let queue_backend = QueueBackend::from_config(config).await?;
    
    Ok((api_db, queue_backend))
}

// 移除旧的 JobStorage 和 push_with_parent! 宏
```

---

## Task 9: Update WorkerState

**Files:**
- Modify: `crates/randimg-core/src/lib.rs`

- [ ] **Step 1: Update WorkerState struct**

```rust
// crates/randimg-core/src/lib.rs

/// WorkerState - 工作节点状态
/// 
/// 设计思路：
/// - 移除 apalis_pool，使用 fang::QueueBackend
/// - 保持其他字段不变，确保向后兼容
/// - job_storage 保留用于任务推送
use crate::task_queue::fang_backend::QueueBackend;

pub struct WorkerState {
    pub db: DatabaseConnection,
    pub config: AppConfig,
    pub oss: OssClient,
    pub queue_backend: QueueBackend,  // 替代 apalis_pool
    pub http_client: reqwest::Client,
    // ... 其他字段 ...
}
```

- [ ] **Step 2: Update spawn_workers macro**

```rust
// crates/randimg-core/src/lib.rs

/// 启动所有 worker
/// 
/// 使用 fang 的 AsyncWorkerPool 替代 apalis 的 worker
macro_rules! spawn_workers {
    ($state:expr) => {
        use fang::{AsyncWorkerPool, AsyncRunnable};
        
        let config = &$state.config;
        let queue = $state.queue_backend.as_queueable();
        
        // 为每种任务类型创建独立的 worker pool
        let pools = vec![
            ("crawl", config.task_concurrency_crawl),
            ("download", config.task_concurrency_download),
            ("color_extract", config.task_concurrency_color_extract),
            ("upload", config.task_concurrency_upload),
            ("accessibility_check", config.task_concurrency_accessibility_check),
            ("discover", config.task_concurrency_discover),
            ("refresh_pixiv_token", config.task_concurrency_refresh_pixiv_token),
        ];
        
        for (task_type, concurrency) in pools {
            let mut pool = AsyncWorkerPool::builder()
                .number_of_workers(concurrency)
                .queue(queue)
                .task_type(task_type)
                .build();
            
            tokio::spawn(async move {
                pool.start().await;
            });
        }
    };
}
```

---

## Task 10: Update Task API Handlers

**Files:**
- Modify: `crates/randimg-core/src/handlers/task.rs`

- [ ] **Step 1: Update to use new tasks table**

```rust
// crates/randimg-core/src/handlers/task.rs

/// 任务管理 API
/// 
/// 设计思路：
/// - 使用自定义 tasks 表替代 apalis_job 表
/// - 保持 API 接口不变，确保向后兼容
/// - 支持任务树查询和状态管理
use crate::db::query::task as task_query;
use crate::db::query::task_tree;
use crate::db::entities::task::{STATUS_PENDING, STATUS_QUEUED, STATUS_RUNNING, STATUS_DONE, STATUS_FAILED, STATUS_KILLED};

/// 列出任务
pub async fn list_tasks(
    State(state): State<Arc<WorkerState>>,
    Query(params): Query<ListTasksParams>,
) -> Result<Json<ListTasksResponse>, Error> {
    let (tasks, total) = task_query::list(
        &state.db,
        params.page.unwrap_or(0),
        params.page_size.unwrap_or(20),
        params.status.as_deref(),
        params.task_type.as_deref(),
    ).await?;
    
    Ok(Json(ListTasksResponse {
        tasks: tasks.into_iter().map(Into::into).collect(),
        total,
        page: params.page.unwrap_or(0),
        page_size: params.page_size.unwrap_or(20),
    }))
}

/// 获取任务详情
pub async fn get_task(
    State(state): State<Arc<WorkerState>>,
    Path(task_id): Path<String>,
) -> Result<Json<TaskResponse>, Error> {
    let task = task_query::find_by_id(&state.db, &task_id)
        .await?
        .ok_or(Error::NotFound)?;
    
    Ok(Json(task.into()))
}

/// 删除任务
pub async fn delete_task(
    State(state): State<Arc<WorkerState>>,
    Path(task_id): Path<String>,
) -> Result<Json<DeleteResponse>, Error> {
    let deleted = task_query::delete_by_id(&state.db, &task_id).await?;
    
    if deleted {
        Ok(Json(DeleteResponse { success: true }))
    } else {
        Err(Error::NotFound)
    }
}

// ... 类似更新其他 API 端点 ...
```

---

## Task 11: Create Tasks Table Migration

**Files:**
- Create: `migration/src/m20260603_001_create_tasks_table.rs`
- Modify: `migration/src/lib.rs`

- [ ] **Step 1: Create migration**

```rust
// migration/src/m20260603_001_create_tasks_table.rs

/// 创建自定义任务表
/// 
/// 设计思路：
/// - 与 fang_tasks 表分离，避免依赖 fang 内部实现
/// - 包含业务相关字段，支持任务树结构
/// - 使用 UUID 或 ULID 作为主键
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Tasks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Tasks::Id).string().not_null().primary_key())
                    .col(ColumnDef::new(Tasks::FangTaskId).big_integer().null())
                    .col(ColumnDef::new(Tasks::TaskType).string().not_null())
                    .col(ColumnDef::new(Tasks::Status).string().not_null())
                    .col(ColumnDef::new(Tasks::ParentId).string().null())
                    .col(ColumnDef::new(Tasks::RootId).string().null())
                    .col(ColumnDef::new(Tasks::CrawlerId).integer().null())
                    .col(ColumnDef::new(Tasks::ImageId).integer().null())
                    .col(ColumnDef::new(Tasks::Params).text().null())
                    .col(ColumnDef::new(Tasks::ErrorMessage).text().null())
                    .col(ColumnDef::new(Tasks::RetryCount).integer().not_null().default(0))
                    .col(ColumnDef::new(Tasks::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Tasks::UpdatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Tasks::CompletedAt).timestamp_with_time_zone().null())
                    .to_owned(),
            )
            .await?;
        
        // 创建索引
        manager
            .create_index(
                Index::create()
                    .table(Tasks::Table)
                    .col(Tasks::Status)
                    .to_owned(),
            )
            .await?;
        
        manager
            .create_index(
                Index::create()
                    .table(Tasks::Table)
                    .col(Tasks::TaskType)
                    .to_owned(),
            )
            .await?;
        
        manager
            .create_index(
                Index::create()
                    .table(Tasks::Table)
                    .col(Tasks::ParentId)
                    .to_owned(),
            )
            .await?;
        
        Ok(())
    }
    
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Tasks::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Tasks {
    Table,
    Id,
    FangTaskId,
    TaskType,
    Status,
    ParentId,
    RootId,
    CrawlerId,
    ImageId,
    Params,
    ErrorMessage,
    RetryCount,
    CreatedAt,
    UpdatedAt,
    CompletedAt,
}
```

- [ ] **Step 2: Register migration**

```rust
// migration/src/lib.rs

mod m20260603_001_create_tasks_table;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            // ... existing migrations ...
            Box::new(m20260603_001_create_tasks_table::Migration),
        ]
    }
}
```

---

## Task 12: Update Server Binary

**Files:**
- Modify: `crates/randimg-server/src/main.rs`

- [ ] **Step 1: Update server initialization**

```rust
// crates/randimg-server/src/main.rs

/// 服务器初始化
/// 
/// 设计思路：
/// - 使用新的 db_backend::init() 分别初始化 API 数据库和队列数据库
/// - 移除 apalis 相关代码
/// - 保持 Axum 路由和中间件不变
use randimg_core::db_backend;

#[tokio::main]
async fn main() -> Result<(), Error> {
    // 初始化配置
    let config = AppConfig::from_env()?;
    
    // 初始化数据库（分别初始化 API 数据库和队列数据库）
    let (api_db, queue_backend) = db_backend::init(&config).await?;
    
    // 创建 WorkerState
    let state = Arc::new(WorkerState {
        db: api_db,
        config,
        oss: OssClient::new()?,
        queue_backend,
        http_client: reqwest::Client::new(),
    });
    
    // 启动 worker
    spawn_workers!(state);
    
    // 启动 Axum 服务器
    let app = Router::new()
        // ... 路由配置 ...
        .with_state(state);
    
    // ... 启动服务器 ...
}
```

---

## Task 13: Update Worker Binary

**Files:**
- Modify: `crates/randimg-worker/src/main.rs`

- [ ] **Step 1: Update worker initialization**

```rust
// crates/randimg-worker/src/main.rs

/// Worker 初始化
/// 
/// 设计思路：
/// - 使用新的 db_backend::init() 分别初始化 API 数据库和队列数据库
/// - 移除 apalis 相关代码
/// - 保持 worker 架构不变
use randimg_core::db_backend;

#[tokio::main]
async fn main() -> Result<(), Error> {
    // 初始化配置
    let config = AppConfig::from_env()?;
    
    // 初始化数据库
    let (api_db, queue_backend) = db_backend::init(&config).await?;
    
    // 创建 WorkerState
    let state = Arc::new(WorkerState {
        db: api_db,
        config,
        oss: OssClient::new()?,
        queue_backend,
        http_client: reqwest::Client::new(),
    });
    
    // 启动 worker
    spawn_workers!(state);
    
    // 等待关闭信号
    tokio::signal::ctrl_c().await?;
    
    Ok(())
}
```

---

## Task 14: Update Error Handling

**Files:**
- Modify: `crates/randimg-core/src/error.rs`

- [ ] **Step 1: Add fang error conversion**

```rust
// crates/randimg-core/src/error.rs

/// 错误处理
/// 
/// 设计思路：
/// - 添加 FangError 到 AppError 的转换
/// - 保持原有的错误处理模式
use fang::FangError;

impl From<FangError> for AppError {
    fn from(err: FangError) -> Self {
        AppError::Internal(format!("Fang error: {}", err))
    }
}
```

---

## Task 15: Update Tests

**Files:**
- Modify: `crates/randimg-core/tests/`

- [ ] **Step 1: Update test fixtures**

```rust
// crates/randimg-core/tests/common/mod.rs

/// 测试工具
/// 
/// 设计思路：
/// - 使用内存 SQLite 数据库进行测试
/// - 分别初始化 API 数据库和队列数据库
/// - 提供测试用的 WorkerState
pub async fn setup_test_state() -> Arc<WorkerState> {
    let config = AppConfig {
        api_database_url: "sqlite::memory:".to_string(),
        queue_database_url: "sqlite::memory:".to_string(),
        // ... 其他配置 ...
    };
    
    let (api_db, queue_backend) = db_backend::init(&config).await.unwrap();
    
    Arc::new(WorkerState {
        db: api_db,
        config,
        oss: OssClient::new().unwrap(),
        queue_backend,
        http_client: reqwest::Client::new(),
    })
}
```

---

## Task 16: Update Documentation

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update README.md**

```markdown
# README.md

## 任务队列

使用 [fang](https://github.com/ayrat555/fang) 作为任务队列后端，支持 SQLite 和 PostgreSQL。

### 配置

```bash
# API 数据库 (SeaORM)
API_DATABASE_URL=sqlite://data/randimg.db?mode=rwc

# 队列数据库 (Fang)
QUEUE_DATABASE_URL=sqlite://data/fang.db?mode=rwc

# 任务调度配置
TASK_MAX_RETRIES=3
TASK_BACKOFF_BASE=2
TASK_POLL_INTERVAL_MS=500
TASK_CONCURRENCY_CRAWL=2
TASK_CONCURRENCY_DOWNLOAD=4
```

### 任务类型

| 任务类型 | 说明 | 默认并发数 |
|---------|------|-----------|
| crawl | 爬虫任务 | 2 |
| download | 下载任务 | 4 |
| color_extract | 颜色提取 | 2 |
| upload | 上传任务 | 2 |
| accessibility_check | 无障碍检查 | 2 |
| discover | 发现任务 | 1 |
| refresh_pixiv_token | 刷新 Pixiv Token | 1 |
```

- [ ] **Step 2: Update CLAUDE.md**

```markdown
# CLAUDE.md

## 任务队列架构

### 数据库分离
- **API 数据库**: 存储业务数据和自定义任务表 (`tasks`)
- **队列数据库**: 存储 fang 内部任务表 (`fang_tasks`)

### 任务状态同步
1. 推送任务时：先插入 `tasks` 表，再推送到 fang 队列
2. 执行任务时：更新 `tasks` 表状态
3. 查询任务时：从 `tasks` 表读取（不直接访问 fang 表）

### 添加新任务类型
1. 在 `task_queue/jobs.rs` 定义任务结构体
2. 实现 `AsyncRunnable` trait
3. 在 `task_queue/handlers.rs` 添加处理器
4. 在 `lib.rs` 的 `spawn_workers!` 宏中注册
```

---

## Commit Strategy

Each task should be committed separately:

```bash
# Task 1: Add fang dependency
git add Cargo.toml crates/randimg-core/Cargo.toml
git commit -m "chore: add fang dependency, remove apalis"

# Task 2: Update config
git add crates/randimg-core/src/config.rs .env.example
git commit -m "feat: add separate database config and task scheduling config"

# Task 3: Create task entity
git add crates/randimg-core/src/db/entities/task.rs
git commit -m "feat: create custom tasks table entity"

# ... etc
```

---

## Verification

After completing all tasks:

1. Run `cargo check` — ensure no compilation errors
2. Run `cargo test -p randimg-core --features db-sqlite,queue-sqlite` — ensure tests pass
3. Run `cargo run -p randimg-server` — ensure server starts
4. Run `cargo run -p randimg-worker` — ensure worker starts
5. Test API endpoints — ensure task management works

---

## Rollback Plan

If migration fails:

1. Revert to apalis dependency
2. Restore original config structure
3. Drop custom tasks table migration
4. Update documentation to reflect rollback
