# Randimg Backend

Pixiv 图片爬取、色彩提取与 API 服务的 Rust 后端。从 Pixiv 爬取图片，通过 CIELAB 色彩空间的 KMeans++ 聚类提取调色板，再通过 HTTP API 对外提供服务。

## 特性

- **Pixiv 爬取** — 用户作品、日榜、收藏（支持标签过滤）三种爬取模式
- **自主发现** — 递归遍历 Pixiv 相关作品 API，可配置跳转深度
- **色彩提取** — KMeans++ 在 CIELAB 空间聚类，输出 10 个调色板色 + 1 个主色，rayon 并行加速
- **色彩搜索** — 支持 RGB / LAB 输入，按色彩相似度检索图片
- **OSS 上传** — 通过 DogeCloud（AWS S3 兼容）上传至云端
- **JWT 认证** — Argon2 密码哈希，`AuthUser` / `OptionalAuthUser` 提取器
- **后台任务队列** — 7 种 Worker，Fang 驱动，可配重试次数与退避策略
- **分布式部署** — 独立 Worker 二进制，API 与队列数据库分离
- **PostgreSQL** — 双数据库架构，API 与队列数据库分离
- **软删除** — 设置 `deleted_at` 和 `is_public = false`
- **优雅关停** — SIGINT / SIGTERM 信号处理

## 技术栈

| 层 | 技术 |
|---|---|
| Web 框架 | Axum 0.8 + Tower |
| 异步运行时 | Tokio 1 |
| ORM | SeaORM 1 |
| 任务队列 | Fang（async PostgreSQL） |
| 数据库 | PostgreSQL，API 与队列分离 |
| 认证 | JWT + Argon2 |
| 图片处理 | `image` + Rayon |
| 云存储 | AWS SDK S3（DogeCloud） |
| 日志 | tracing + tracing-subscriber |

## 快速开始

### 1. 环境准备

需要 Rust 工具链（Edition 2024）。复制并编辑环境变量：

```bash
cp .env.example .env
# 编辑 .env，至少设置 SECRET_KEY（不能为 "change-me"）
# 确保 API_DATABASE_URL 和 QUEUE_DATABASE_URL 可连
```

### 2. 构建与运行

```bash
cargo build
cargo run -p randimg-server
```

### 3. 分布式部署（可选）

Worker 二进制不依赖 Axum，可单独部署：

```bash
# 在 Worker 机器上
cargo run -p randimg-worker
```

### 4. 创建管理员

```bash
cargo run -p randimg-server --bin create-admin
```

### 5. 运行测试

```bash
# 跳过色彩测试（性能开销大）
cargo test -p randimg-core -- --skip kmeans --skip extract_theme --skip histogram --skip palette --skip primary_color --skip lab_round
```

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `API_DATABASE_URL` | `postgres://localhost/randimg` | API 数据库连接串（SeaORM） |
| `QUEUE_DATABASE_URL` | `postgres://localhost/randimg_queue` | 队列数据库连接串（Fang，PostgreSQL） |
| `SECRET_KEY` | **必填** | JWT 签名密钥 |
| `JWT_EXPIRE_MINUTES` | `60` | JWT 过期时间（分钟） |
| `CDN_BASE_URL` | `https://cdn.example.com/` | 图片 CDN 前缀 |
| `IMAGE_DIR` | `./images` | 本地图片存储路径 |
| `SERVER_ADDR` | `0.0.0.0:8000` | 监听地址（TCP 或 Unix Socket） |
| `PIXIV_REFRESH_TOKEN` | — | Pixiv API refresh token |
| `PIXIV_PROXY` | — | Pixiv API 代理 |
| `RUST_LOG` | `randimg_core=info,tower_http=info` | 日志级别 |
| `LOG_DIR` | `./logs` | 日志目录 |
| `LOG_JSON` | `false` | JSON 格式日志 |
| `MAX_DISCOVER_HOPS` | `3` | 自主发现最大跳转深度 |
| `DISCOVER_SEED_LIMIT` | `5` | 每批发现种子数 |
| `DOGECLOUD_ACCESS_KEY` | — | DogeCloud Console API Key |
| `DOGECLOUD_SECRET_KEY` | — | DogeCloud Console API Secret |
| `DOGECLOUD_S3_BUCKET` | — | S3 存储桶名 |
| `DOGECLOUD_S3_ENDPOINT` | — | S3 端点 URL |
| `TASK_MAX_RETRIES` | `3` | 任务最大重试次数 |
| `TASK_BACKOFF_BASE` | `2` | 重试退避基数（秒） |
| `TASK_POLL_INTERVAL_MS` | `500` | 队列轮询间隔（毫秒） |
| `TASK_DEFAULT_TIMEOUT_SECS` | `300` | 任务默认超时（秒） |
| `TASK_CONCURRENCY_CRAWL` | `2` | crawl 并发数 |
| `TASK_CONCURRENCY_DOWNLOAD` | `4` | download 并发数 |
| `TASK_CONCURRENCY_COLOR_EXTRACT` | `2` | color_extract 并发数 |
| `TASK_CONCURRENCY_UPLOAD` | `2` | upload 并发数 |
| `TASK_CONCURRENCY_ACCESSIBILITY_CHECK` | `2` | accessibility_check 并发数 |
| `TASK_CONCURRENCY_DISCOVER` | `1` | discover 并发数 |
| `TASK_CONCURRENCY_REFRESH_PIXIV_TOKEN` | `1` | refresh_pixiv_token 并发数 |

监听地址支持三种格式：
- `0.0.0.0:8000` — TCP
- `http://127.0.0.1:8000` — TCP（自动去掉 scheme）
- `unix:///run/randimg.sock` — Unix Socket

## API

### 公开接口

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/health` | 健康检查 |
| GET | `/` | 随机图片（query: format, local, ratio, width, height, author, tags） |
| GET | `/image/{id}` | 图片详情 |
| GET | `/list` | 分页图片列表 |
| GET | `/color/search` | 色彩搜索（RGB / LAB） |
| GET | `/statistic` | 统计数据 |
| GET | `/tags` | 标签列表 |
| GET | `/authors` | 作者列表 |
| GET | `/authors/{id}` | 作者详情 |

### 认证接口

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/token` | 登录，返回 JWT |

### 管理接口（需 AuthUser）

| 方法 | 路径 | 说明 |
|------|------|------|
| PATCH | `/image/{id}` | 更新图片 |
| DELETE | `/image/{id}` | 软删除图片 |
| PATCH | `/tags/{id}` | 更新标签 |
| DELETE | `/tags/{id}` | 删除标签 |
| GET/POST | `/crawler` | 爬虫任务管理 |
| POST | `/crawler/discover` | 触发自主发现 |
| GET/POST | `/pixiv-credential` | Pixiv 凭证管理 |
| POST | `/pixiv-credential/{id}/refresh` | 刷新 Token |

### 任务管理接口（需 AuthUser）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/tasks` | 后台任务列表（支持 task_type/status/limit/offset 过滤） |
| GET | `/tasks/{id}` | 获取单个任务详情 |
| DELETE | `/tasks/{id}` | 删除单个任务 |
| POST | `/tasks/clean` | 按状态批量清理任务（flags: completed/failed/cancelled/pending/running/all） |
| DELETE | `/tasks/pending` | 删除所有等待中的任务（支持 task_type 过滤） |
| GET | `/tasks/roots` | 获取根任务（无父任务的任务） |
| GET | `/tasks/{id}/tree` | 获取任务树（递归子任务） |
| GET | `/tasks/{id}/subtasks` | 获取直接子任务列表 |
| DELETE | `/tasks/{id}/subtasks` | 中断所有等待中的子任务 |

完整 API 文档见 [docs/api.md](docs/api.md)。

## 架构

```
Cargo.toml                    # 虚拟 Workspace 根
├── crates/randimg-core/      # 库：WorkerState、所有共享代码
│   ├── src/
│   │   ├── lib.rs            # WorkerState 定义、模块导出
│   │   ├── config.rs         # AppConfig（环境变量，含 API/队列数据库分离）
│   │   ├── error.rs          # AppError 统一错误处理
│   │   ├── db_backend.rs     # 队列后端抽象（Fang AsyncRunnable）
│   │   ├── auth/             # JWT + Argon2 认证
│   │   ├── color/            # KMeans++ 色彩提取（CIELAB 空间）
│   │   ├── db/
│   │   │   ├── entities/     # SeaORM 实体模型
│   │   │   └── query/        # 数据库查询函数
│   │   ├── dogecloud/        # DogeCloud OSS 集成
│   │   ├── handlers/         # Axum 路由处理器
│   │   ├── pixiv/            # Pixiv API 客户端封装
│   │   └── task_queue/       # 任务定义 + Worker 处理器
│   └── tests/
├── crates/randimg-server/    # Binary：Axum HTTP 服务器
│   └── src/main.rs           # ServerState、路由、优雅关停
├── crates/randimg-worker/    # Binary：无头 Worker（无 Axum）
│   └── src/main.rs           # WorkerState、Monitor 管理全部 7 个 Worker
└── migration/
    └── src/                  # SeaORM 迁移（自动执行）
```

分层调用：`handlers → db/query → db/entities`

### 后台 Worker

| Worker | 处理函数 | 默认并发数 | 环境变量 |
|--------|----------|------------|----------|
| crawl | `handle_crawl` | 2 | `TASK_CONCURRENCY_CRAWL` |
| download | `handle_download` | 4 | `TASK_CONCURRENCY_DOWNLOAD` |
| color-extract | `handle_color_extract` | 2 | `TASK_CONCURRENCY_COLOR_EXTRACT` |
| upload | `handle_upload` | 2 | `TASK_CONCURRENCY_UPLOAD` |
| accessibility-check | `handle_accessibility_check` | 2 | `TASK_CONCURRENCY_ACCESSIBILITY_CHECK` |
| discover | `handle_discover` | 1 | `TASK_CONCURRENCY_DISCOVER` |
| refresh-pixiv-token | `handle_refresh_pixiv_token` | 1 | `TASK_CONCURRENCY_REFRESH_PIXIV_TOKEN` |

### 任务队列

Fang 驱动的异步任务队列，API 数据库与队列数据库分离：

- **API 数据库**（`API_DATABASE_URL`）：SeaORM 管理业务数据 + `tasks` 表记录任务元信息
- **队列数据库**（`QUEUE_DATABASE_URL`）：Fang 管理任务调度，PostgreSQL 后端
- **任务推送流程**：`query::task::create()` → `queue.insert_task()` → `query::task::link_fang_task()`
- **Worker 模式**：实现 `AsyncRunnable` trait，配合 `#[typetag::serde]` + `#[async_trait]`

任务重试通过 `TASK_MAX_RETRIES` 和 `TASK_BACKOFF_BASE` 配置指数退避策略。

## License

MIT © 2026 Jiaqi Cheng
