# 任务流设计文档

## 概述

Randimg 使用 Fang 异步任务队列处理后台任务。任务分为根任务和子任务，形成树状层级结构。本文档描述任务的触发流程和当前设计。

## 架构组件

### 1. 双表设计

系统维护两张任务表：

| 表名 | 用途 | 管理方 |
|------|------|--------|
| `tasks` | 自定义任务记录，存储业务字段（crawler_id, image_id 等） | SeaORM |
| `fang_tasks` | Fang 队列任务，存储序列化的 job struct | Fang |

关联关系：`tasks.fang_task_id` → `fang_tasks.id`（UUID 字符串）

### 2. Worker 池

每种任务类型有独立的 Worker 池，通过 `task_type` 字符串匹配：

```rust
// crates/randimg-core/src/lib.rs
let pool_configs: &[(&str, u32)] = &[
    ("crawl", state.config.task_concurrency_crawl),           // 默认 2
    ("download", state.config.task_concurrency_download),      // 默认 4
    ("color_extract", state.config.task_concurrency_color_extract), // 默认 2
    ("upload", state.config.task_concurrency_upload),          // 默认 2
    ("accessibility_check", state.config.task_concurrency_accessibility_check), // 默认 2
    ("discover", state.config.task_concurrency_discover),      // 默认 1
    ("refresh_pixiv_token", state.config.task_concurrency_refresh_pixiv_token), // 默认 1
];
```

## 任务类型

### 根任务

| 任务类型 | Job Struct | 触发方式 |
|----------|------------|----------|
| `crawl` | `CrawlJob` | API 调用 `POST /api/crawlers` |
| `discover` | `DiscoverJob` | CrawlJob 完成后自动触发 |

### 子任务（由根任务触发）

| 任务类型 | Job Struct | 触发条件 |
|----------|------------|----------|
| `download` | `DownloadJob` | CrawlJob/DiscoverJob 发现新图片时 |
| `color_extract` | `ColorExtractJob` | DownloadJob 完成后 |
| `upload` | `UploadJob` | DownloadJob 完成后 |
| `accessibility_check` | `AccessibilityCheckJob` | DownloadJob 完成后 |

## 任务触发流程

### 1. CrawlJob 触发流程

```
API: POST /api/crawlers
    │
    ├─ 创建 crawler 记录
    ├─ 构造 CrawlJob struct（包含 task_id）
    └─ push_task(&crawl_job, "crawl", ...)
            │
            ├─ tasks 表：INSERT（status: pending）
            ├─ fang_tasks 表：INSERT（序列化的 CrawlJob）
            └─ tasks 表：UPDATE（status: queued, fang_task_id = UUID）
```

### 2. CrawlJob 执行流程

```
Fang Worker 反序列化 CrawlJob
    │
    ├─ AsyncRunnable::run()
    │   ├─ 更新 status → running
    │   └─ 调用 handlers::handle_crawl()
    │
    └─ handle_crawl()
        │
        ├─ 根据 crawl_type 分发：
        │   ├─ crawl_ranking()   // crawl_type = 0
        │   ├─ crawl_user()      // crawl_type = 1
        │   └─ crawl_bookmarks() // crawl_type = 2
        │
        ├─ 每个 illust 调用 save_illust()
        │   │
        │   ├─ 过滤：illust_type_filter, exclude_r18, exclude_ai
        │   ├─ 创建/更新 author 记录
        │   ├─ 创建 image 记录（每页一个）
        │   ├─ 创建 tag 关联
        │   └─ 返回 Vec<DownloadInfo>（新创建的图片）
        │
        ├─ 对每个 DownloadInfo 创建 DownloadJob：
        │   push_task(&download_job, "download", ...)
        │
        └─ crawl 完成后：
            │
            ├─ 更新 crawler 状态
            └─ 如果 !disable_discover：
                push_task(&discover_job, "discover", ...)
```

### 3. DownloadJob 执行流程

```
Fang Worker 反序列化 DownloadJob
    │
    ├─ AsyncRunnable::run()
    │   ├─ 更新 status → running
    │   └─ 调用 handlers::handle_download()
    │
    └─ handle_download()
        │
        ├─ 检查文件是否已存在
        │   ├─ 已存在：标记 downloaded，回写尺寸，spawn_downstream_children()
        │   └─ 不存在：下载文件
        │
        ├─ 下载完成后：
        │   ├─ 标记 downloaded
        │   └─ 回写实际尺寸
        │
        └─ spawn_downstream_children()
            │
            ├─ push_task(&color_job, "color_extract", ...)
            ├─ push_task(&upload_job, "upload", ...)
            └─ push_task(&a11y_job, "accessibility_check", ...)
```

### 4. DiscoverJob 执行流程

```
Fang Worker 反序列化 DiscoverJob
    │
    ├─ AsyncRunnable::run()
    │   ├─ 更新 status → running
    │   └─ 调用 handlers::handle_discover()
    │
    └─ handle_discover()
        │
        ├─ 查找 discover seeds（按 popularity/views/bookmarks/random）
        ├─ 对每个 seed 调用 Pixiv illust_related API
        ├─ 对每个 related illust 调用 save_illust()
        │   └─ 创建 DownloadJob
        │
        └─ 如果 hop < max_hops：
            push_task(&next_discover_job, "discover", ...)  // hop + 1
```

## 任务层级结构

### 典型任务树

```
crawl (根任务, parent_id=None, root_id=None)
├── download (子任务 1, parent=crawl, root=crawl)
│   ├── color_extract (孙任务, parent=download#1, root=crawl)
│   ├── upload (孙任务, parent=download#1, root=crawl)
│   └── accessibility_check (孙任务, parent=download#1, root=crawl)
├── download (子任务 2, parent=crawl, root=crawl)
│   ├── color_extract
│   ├── upload
│   └── accessibility_check
└── discover (子任务 3, parent=crawl, root=crawl)
    ├── download (discover 发现的图片, parent=discover, root=crawl)
    │   ├── color_extract
    │   ├── upload
    │   └── accessibility_check
    └── discover (下一跳，hop + 1, parent=discover, root=crawl)
        └── ...
```

### parent_id 与 root_id 设计

| 字段 | 含义 | 用途 |
|------|------|------|
| `parent_id` | 直接父任务 ID | 树状结构遍历 |
| `root_id` | 根任务 ID | 扁平查询所有子任务 |

**当前实现**：
- CrawlJob: `parent_id = None`, `root_id = None`（根任务）
- DownloadJob: `parent_id = crawl_job_id`, `root_id = crawl_job_id`
- ColorExtractJob/UploadJob/AccessibilityCheckJob: `parent_id = download_task_id`, `root_id = crawl_job_id`
- DiscoverJob: `parent_id = crawl_job_id`, `root_id = crawl_job_id`（通过 `root_job_id` 字段传播）

**设计保证**：
- `parent_id` 指向直接触发该子任务的父任务
- `root_id` 始终指向根任务（crawl job），用于 API 扁平树分页查询
- DownloadJob 是 color_extract/upload/accessibility_check 的父任务
- DiscoverJob 通过 `root_job_id` 字段保持与原始 crawl 的连接

## push_task() 方法签名

```rust
// crates/randimg-core/src/task_queue/fang_backend.rs
pub async fn push_task(
    &self,
    task: &(dyn AsyncRunnable + Send + Sync),  // 实际的 job struct
    task_type: &str,                            // 任务类型标识
    metadata: JsonValue,                        // 任务参数 JSON
    db: &DatabaseConnection,                    // SeaORM 数据库连接
    parent_id: Option<&str>,                    // 直接父任务 ID
    root_id: Option<&str>,                      // 根任务 ID（crawl job）
    crawler_id: Option<i32>,                    // 关联爬虫 ID
    image_id: Option<i32>,                      // 关联图片 ID
    task_id: Option<&str>,                      // 自定义任务 ID（可选）
) -> Result<String, String>
```

## 状态流转

```
pending → queued → running → done
                      ↓
                   failed
```

| 状态 | 含义 | 设置时机 |
|------|------|----------|
| `pending` | 任务已创建，等待入队 | `query::task::create()` |
| `queued` | 任务已入 fang 队列 | `query::task::link_fang_task()` |
| `running` | 任务正在执行 | `AsyncRunnable::run()` 开始时 |
| `done` | 任务完成 | `AsyncRunnable::run()` 成功时 |
| `failed` | 任务失败 | `AsyncRunnable::run()` 失败时 |

## 关键代码位置

| 文件 | 内容 |
|------|------|
| `crates/randimg-core/src/task_queue/jobs.rs` | Job struct 定义 + AsyncRunnable 实现 |
| `crates/randimg-core/src/task_queue/handlers.rs` | 业务逻辑处理函数 |
| `crates/randimg-core/src/task_queue/fang_backend.rs` | QueueBackend 封装 |
| `crates/randimg-core/src/lib.rs` | spawn_workers() 启动 Worker 池 |
| `crates/randimg-core/src/handlers/crawler.rs` | API 触发 CrawlJob |
| `crates/randimg-core/src/db/query/task.rs` | 任务 CRUD + 状态更新 |

## 重试机制

### Fang 内置重试

所有任务类型使用 fang 的 `AsyncRunnable` trait 内置重试机制：

```rust
fn max_retries(&self) -> i32 { self.max_retries }
fn backoff(&self, attempt: u32) -> u32 { u32::pow(self.backoff_base, attempt) }
```

### 重试策略

| 任务类型 | max_retries | backoff_base | 说明 |
|----------|-------------|-------------|------|
| crawl | 3 | 2 | 网络请求，指数退避 |
| download | 3 | 2 | 网络请求，指数退避 |
| color_extract | 0 | 2 | CPU 密集，不重试 |
| upload | 3 | 2 | 网络请求，指数退避 |
| accessibility_check | 3 | 2 | 网络请求，指数退避 |
| discover | 3 | 2 | 网络请求，指数退避 |
| refresh_pixiv_token | 3 | 2 | 网络请求，指数退避 |

### 配置参数

通过环境变量配置：

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `TASK_MAX_RETRIES` | 3 | 最大重试次数 |
| `TASK_BACKOFF_BASE` | 2 | 指数退避基数（秒） |

## 当前问题

### ~~1. 任务树扁平化~~ ✅ 已修复

所有下游任务的 `parent_id` 现在正确指向直接父任务（download task），`root_id` 始终指向根任务（crawl job）。

### ~~2. DiscoverJob 触发时机~~ ✅ 无需修改

下载任务仅包含图片文件下载，metadata 已在 `save_illust()` 中就绪。

### ~~3. 错误处理~~ ✅ 已修复

子任务创建失败时通过 `?` 传播错误，fang 自动重试父任务。
