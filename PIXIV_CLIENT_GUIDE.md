# pixiv-client 使用指南与 Worker 审计

> 基于 pixiv-client v1.2.0 (https://github.com/modenicheng/pixiv-api)

## 目录

1. [Crate 概览](#1-crate-概览)
2. [核心类型](#2-核心类型)
3. [认证流程](#3-认证流程)
4. [API 方法参考](#4-api-方法参考)
5. [分页模式](#5-分页模式)
6. [错误处理](#6-错误处理)
7. [下载器模块](#7-下载器模块)
8. [后端集成模式](#8-后端集成模式)
9. [Worker 审计结果](#9-worker-审计结果)
10. [改进建议](#10-改进建议)

---

## 1. Crate 概览

| 字段 | 值 |
|------|-----|
| 名称 | `pixiv-client` |
| 版本 | 1.2.0 |
| 仓库 | https://github.com/modenicheng/pixiv-api |
| 文档 | https://docs.rs/pixiv-client/latest/pixiv_client/ |
| 许可证 | MIT |
| 异步运行时 | tokio |
| HTTP 客户端 | reqwest 0.13 (rustls) |

**Feature flags:**
- `gfw-bypass` — DNS-over-HTTPS 绕过 GFW（后端未启用）

**依赖:** reqwest, serde, chrono, sha2, md-5, base64, rand 0.9, url, futures-util

---

## 2. 核心类型

### 2.1 PixivApi

客户端主结构体，所有 API 调用的入口。

```rust
pub struct PixivApi {
    client: Client,
    tokens: Mutex<(Option<String>, Option<String>, Option<u64>)>, // (access, refresh, user_id)
    config: Config,
    custom_headers: Arc<Mutex<HeaderMap>>,
}
```

**构造方法:**
```rust
// 默认客户端
let api = PixivApi::new();

// 自定义配置（代理、超时）
let client_config = ClientConfig {
    proxy: Some("http://127.0.0.1:7890".to_string()),
    timeout: Duration::from_secs(30),
    ..Default::default()
};
let api = PixivApi::with_config(Config::default(), client_config);
```

### 2.2 ApiResponse<T>

混合响应：类型化数据 + 原始 JSON。如果 Pixiv 修改 API 导致反序列化失败，`data` 为 `None` 但 `raw` 始终可用。

```rust
pub struct ApiResponse<T> {
    pub data: Option<T>,      // 类型化结构体（反序列化失败时为 None）
    pub raw: serde_json::Value, // 原始 JSON（始终可用）
}
```

**使用模式:**
```rust
let resp = api.illust_ranking(None, None, None).await?;

// 方式 1：直接解包（data 为 None 时 panic）
let data = resp.data.unwrap();

// 方式 2：安全处理
if let Some(data) = resp.data {
    println!("Got {} illusts", data.illusts.len());
}

// 方式 3：始终可用的原始 JSON
println!("Raw: {}", resp.raw);
```

### 2.3 PixivError

```rust
pub enum PixivError {
    Auth(String),           // 认证失败
    Request(reqwest::Error), // HTTP 请求错误
    Status(StatusCode),     // HTTP 状态码错误（如 401, 429）
    Parse(serde_json::Error), // JSON 解析错误
    Download(String),       // 下载失败
    Io(std::io::Error),     // IO 错误
    Other(String),          // 其他错误
}

impl PixivError {
    pub fn is_auth_error(&self) -> bool; // 是否为 401 认证错误
}
```

### 2.4 关键 Model 类型

```rust
// 插画
pub struct Illust {
    pub id: u64,
    pub title: String,
    pub user: Option<UserPreview>,
    pub tags: Option<Vec<Tag>>,
    pub image_urls: Option<ImageUrls>,
    pub meta_single_page: Option<MetaSinglePage>,
    pub meta_pages: Option<Vec<MetaPage>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub total_view: Option<u64>,
    pub total_bookmarks: Option<u64>,
    pub total_comments: Option<u64>,
    pub create_date: Option<DateTime<Utc>>,
}

// 图片 URL 集合
pub struct ImageUrls {
    pub square_medium: Option<String>,
    pub medium: Option<String>,
    pub large: Option<String>,
    pub original: Option<String>,
}

// 单页插画的原始图片 URL
pub struct MetaSinglePage {
    pub original_image_url: Option<String>,
}

// 多页插画的每一页
pub struct MetaPage {
    pub image_urls: Option<ImageUrls>,
}

// 标签
pub struct Tag {
    pub name: String,
    pub translated_name: Option<String>,
    pub added_by_uploaded_user: Option<bool>,
}

// 用户预览
pub struct UserPreview {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub account: Option<String>,
}
```

---

## 3. 认证流程

### 3.1 主要认证方式：Refresh Token

```rust
let api = PixivApi::new();
api.auth("your_refresh_token").await?;
// 现在可以调用需要认证的 API
```

`auth()` 会：
1. 用 refresh_token 向 Pixiv OAuth 服务器换取新的 access_token 和 refresh_token
2. 将新 token 存储在内部 `Mutex` 中
3. Pixiv 的 refresh_token 是可旋转的——每次 auth() 调用可能返回不同的 refresh_token

### 3.2 手动设置 Token

```rust
// 从数据库恢复已有的 token（不需要网络调用）
api.set_auth("access_token", "refresh_token", 12345).await;
```

### 3.3 显式 Token 刷新（v1.2.0+）

```rust
// 当收到 401 错误时，显式调用
match api.some_method().await {
    Err(e) if e.is_auth_error() => {
        api.refresh_token().await?;  // 刷新 token
        api.some_method().await?;    // 重试
    }
    other => other?,
}
```

`refresh_token()` 方法：
- 用存储的 refresh_token 向 Pixiv 换取新的 access_token 和 refresh_token
- 新 token 自动存储在内部 Mutex 中
- 如果没有 refresh_token，返回 `PixivError::Auth("no refresh token available")`

**重要变更（v1.2.0）:** 不再自动重试 401。旧版本会在收到 401 时自动调用 refresh_token 并重试，v1.2.0 返回 `Err(PixivError::Status(401))`，由调用者决定是否刷新。

### 3.4 读取当前 Token

```rust
let access: Option<String> = api.access_token().await;
let refresh: Option<String> = api.current_refresh_token().await;
let user_id: Option<u64> = api.user_id().await;
let is_auth: bool = api.is_authenticated().await;
```

### 3.5 OAuth2 PKCE 获取 Refresh Token

首次获取 refresh_token 需要完整的 OAuth2 PKCE 流程（参见 `examples/get_token.rs`）：

1. 生成 PKCE code_verifier / code_challenge
2. 打开 `https://app-api.pixiv.net/web/v1/login?code_challenge=...&code_challenge_method=S256&client=pixiv-android`
3. 用户登录并授权
4. 从回调 URL 提取 `code`
5. 用 code 换取 tokens：`https://oauth.secure.pixiv.net/auth/token`

---

## 4. API 方法参考

### 4.1 插画相关

| 方法 | 签名 | 端点 | 需要认证 |
|------|------|------|----------|
| `illust_detail` | `(illust_id: u64) -> ApiResponse<IllustDetail>` | `GET /v1/illust/detail` | ✅ |
| `illust_ranking` | `(mode, date, offset) -> ApiResponse<IllustRankingResult>` | `GET /v1/illust/ranking` | ✅ |
| `illust_related` | `(illust_id: u64) -> ApiResponse<IllustRelatedResult>` | `GET /v2/illust/related` | ✅ |
| `illust_recommended` | `() -> ApiResponse<IllustRecommendedResult>` | `GET /v1/illust/recommended` | ✅ |
| `illust_follow` | `(restrict) -> ApiResponse<IllustFollowResult>` | `GET /v2/illust/follow` | ✅ |
| `illust_new` | `() -> ApiResponse<IllustNewResult>` | `GET /v1/illust/new` | ✅ |
| `illust_comments` | `(illust_id, offset) -> ApiResponse<IllustCommentsResult>` | `GET /v1/illust/comments` | ✅ |
| `illust_bookmark_detail` | `(illust_id) -> ApiResponse<IllustBookmarkDetailResult>` | `GET /v2/illust/bookmark/detail` | ✅ |
| `illust_bookmark_add` | `(illust_id, restrict, tags) -> ApiResponse<Value>` | `POST /v2/illust/bookmark/add` | ✅ |
| `illust_bookmark_delete` | `(illust_id) -> ApiResponse<Value>` | `POST /v1/illust/bookmark/delete` | ✅ |
| `refresh_token` | `() -> Result<()>` | 内部调用 | N/A |

**参数说明:**
- `mode`: `"day"`, `"week"`, `"month"`, `"day_male"`, `"day_female"`, `"week_original"`, `"week_rookie"`
- `date`: 格式 `"YYYY-MM-DD"`，`None` 表示当天
- `offset`: 分页偏移量（`u32`），`None` 从头开始
- `restrict`: `"public"` 或 `"private"`

### 4.2 用户相关

| 方法 | 签名 | 端点 |
|------|------|------|
| `user_detail` | `(user_id: u64) -> ApiResponse<UserDetail>` | `GET /v1/user/detail` |
| `user_illusts` | `(user_id, type, offset) -> ApiResponse<UserIllustsResult>` | `GET /v1/user/illusts` |
| `user_bookmarks_illust` | `(user_id, restrict, max_bookmark_id, tag)` | `GET /v1/user/bookmarks/illust` |
| `user_following` | `(user_id, restrict, offset)` | `GET /v1/user/following` |
| `user_follower` | `(user_id, offset)` | `GET /v1/user/follower` |

**参数说明:**
- `type`: `"illust"`, `"manga"`, `"novel"`（`user_illusts` 专用）
- `max_bookmark_id`: 游标分页用（从上一页的 `next_url` 提取）

### 4.3 搜索相关

| 方法 | 签名 | 端点 |
|------|------|------|
| `search_illust` | `(word, sort, duration, search_target, offset)` | `GET /v1/search/illust` |
| `search_user` | `(word, offset)` | `GET /v1/search/user` |
| `trending_tags_illust` | `() -> ApiResponse<TrendingTagsResult>` | `GET /v1/trending-tags/illust` |

**搜索枚举:**
```rust
pub enum SearchSort { DateDesc, DateAsc, PopularDesc, PopularMaleDesc, PopularFemaleDesc }
pub enum SearchDuration { WithinLastDay, WithinLastWeek, WithinLastMonth, None }
pub enum SearchTarget { PartialMatchForTags, ExactMatchForTags, TitleAndCaption, Keyword }
```

### 4.4 小说相关（后端未使用）

| 方法 | 签名 | 端点 |
|------|------|------|
| `novel_detail` | `(novel_id: u64)` | `GET /v2/novel/detail` |
| `novel_text` | `(novel_id: u64)` | `GET /v1/novel/text` |
| `novel_recommended` | `()` | `GET /v1/novel/recommended` |

### 4.5 其他

| 方法 | 签名 | 端点 |
|------|------|------|
| `ugoira_metadata` | `(illust_id: u64)` | `GET /v1/ugoira/metadata` |

---

## 5. 分页模式

### 5.1 偏移量分页（Offset-based）

用于 `illust_ranking`, `user_illusts`, `search_illust` 等。

```rust
let mut offset = 0u32;
loop {
    let resp = api.illust_ranking(Some("day"), None, Some(offset)).await?;
    let data = resp.data.unwrap();
    
    if data.illusts.is_empty() || data.next_url.is_none() {
        break;
    }
    
    // 处理 illusts...
    
    offset += data.illusts.len() as u32;
}
```

### 5.2 游标分页（Cursor-based）

用于 `user_bookmarks_illust`。通过 `next_url` 中的 `max_bookmark_id` 参数实现。

```rust
let mut max_bookmark_id: Option<u64> = None;
loop {
    let resp = api.user_bookmarks_illust(user_id, Some("public"), max_bookmark_id, None).await?;
    let data = resp.data.unwrap();
    
    if data.illusts.is_empty() {
        break;
    }
    
    // 处理 illusts...
    
    // 从 next_url 提取 max_bookmark_id
    if let Some(next_url) = &data.next_url {
        max_bookmark_id = extract_max_bookmark_id(next_url);
    } else {
        break;
    }
}
```

**注意:** crate 提供了 `pixiv_client::models::common::parse_next_url()` 函数来解析 next_url 的查询参数，但后端使用了自定义的 `extract_param_from_url()`。

---

## 6. 错误处理

### 6.1 错误匹配模式

```rust
match api.some_method().await {
    Ok(resp) => { /* 处理响应 */ }
    Err(e) if e.is_auth_error() => {
        // 401: token 过期，需要刷新
        api.refresh_token().await?;
        // 重试
    }
    Err(e) => {
        // 其他错误：网络、解析、限流等
        return Err(e.into());
    }
}
```

### 6.2 限流处理

crate 没有内置限流处理。如果 Pixiv 返回 429 (Too Many Requests)，会得到 `PixivError::Status(429)`。建议：

```rust
Err(e) if matches!(e, PixivError::Status(s) if s == 429) => {
    tokio::time::sleep(Duration::from_secs(60)).await;
    // 重试
}
```

---

## 7. 下载器模块

### 7.1 DownloadManager

crate 内置了并发下载器，支持重试和进度回调：

```rust
use pixiv_client::downloader::{DownloadManager, DownloadTask, ProgressEvent, resolve_download_tasks};

// 创建下载管理器
let dm = DownloadManager::new(reqwest::Client::new(), "./images");

// 从 Illust 解析下载任务（处理单页/多页）
let tasks = resolve_download_tasks(&illust, "original", None);

// 带重试和进度的批量下载（4 并发，4 次重试）
// 重试退避: 0ms, 1s, 2s, 4s
let results = dm.download_all(&tasks, 4, |evt| match evt {
    ProgressEvent::Started { filename, total_bytes } => { /* ... */ }
    ProgressEvent::Chunk { filename, bytes_downloaded } => { /* ... */ }
    ProgressEvent::Finished { filename, path } => { /* ... */ }
    ProgressEvent::Failed { filename, error, attempt } => { /* ... */ }
}).await;

// 单次下载（无重试）
let path = dm.download(url, filename).await?;
```

### 7.2 URL 解析

```rust
use pixiv_client::downloader::url_for_size;

// 获取最佳图片 URL（original > large > medium 的回退链）
let url = url_for_size(&illust.image_urls, "original");
```

### 7.3 resolve_download_tasks

自动处理单页和多页插画：

```rust
// 下载所有页面
let tasks = resolve_download_tasks(&illust, "original", None);

// 只下载第 0 和第 2 页
let tasks = resolve_download_tasks(&illust, "original", Some(&[0, 2]));
```

**后端使用方式:** `handle_download` 使用 `DownloadManager::download()` 进行单张下载。`download_all()` 未使用 — 重试由 Apalis 统一管理。

**注意:** `dm.download()` 是单次下载，没有内置重试。重试由 Apalis `RetryPolicy` 管理。

---

## 8. 后端集成模式

### 8.1 文件结构

```
src/pixiv/mod.rs          — PixivApi 封装层（create_api, auth_with_credential, recover_auth, persist_tokens）
src/task_queue/handlers.rs — 所有 Pixiv API 调用（7 个 handler）
src/task_queue/jobs.rs     — Job 结构体定义（包括 RefreshPixivTokenJob）
src/handlers/pixiv_credential.rs — 凭证 CRUD HTTP 端点
src/db/query/pixiv_credential.rs — 凭证数据库操作
src/db/entities/pixiv_credential.rs — 凭证实体定义
```

### 8.2 认证流程

```
create_api(proxy, accept_lang)
    ↓
auth_with_credential(api, cred, db)
    ├─ 有 access_token → set_auth()（无网络调用）
    └─ 无 access_token → auth() + persist_tokens()
    ↓
API 调用
    ├─ 成功 → 处理响应
    └─ 401 → recover_auth() → 重试一次
```

### 8.3 凭证生命周期

1. **创建**: 通过 `POST /pixiv-credential` 或启动时从 `PIXIV_REFRESH_TOKEN` 环境变量自动创建
2. **使用**: `find_one_active_random()` 随机选择一个 `status=ACTIVE` 的凭证
3. **刷新**:
   - 被动: 收到 401 时 `recover_auth()` 自动刷新
   - 主动: 启动时检查 `last_refreshed_at` 超过 50 分钟则提交 `RefreshPixivTokenJob`
   - 手动: `POST /pixiv-credential/{id}/refresh`
4. **持久化**: 每次 auth/refresh 后将新 token 写回数据库

---

## 9. Worker 审计结果

### 审计范围

审计了所有使用 pixiv-client 的 worker：

| Worker | 文件 | 行号 | 使用的 API 方法 |
|--------|------|------|-----------------|
| handle_crawl | handlers.rs | 20-84 | 分发到 crawl_ranking/user/bookmarks |
| crawl_ranking | handlers.rs | 86-152 | `illust_ranking()` |
| crawl_user | handlers.rs | 154-225 | `user_illusts()` |
| crawl_bookmarks | handlers.rs | 227-301 | `user_bookmarks_illust()` |
| handle_discover | handlers.rs | 730-835 | `illust_related()` |
| handle_refresh_pixiv_token | handlers.rs | 838-893 | `auth()`, `current_refresh_token()`, `access_token()` |
| handle_download | handlers.rs | 526-598 | `DownloadManager::download()` |

---

### D1: 已修复 ✅ — 使用内置下载器

**位置:** `handlers.rs:559-585`

**现状:** 已使用 `DownloadManager::download()` 进行单张图片下载。自动配置 Referer header，Apalis 管理重试。

**设计决策:**
- 使用 `dm.download()`（单张）而非 `dm.download_all()`（批量）— 单张接口自由度更大，便于统一管理重试
- 重试由 Apalis `RetryPolicy::retries(3)` 处理，不由 downloader 内置重试处理
- 子目录创建手动处理（crate 只创建 `output_dir`，不创建嵌套目录）

---

### D2: 未使用 parse_next_url ⚠️ 轻微

**位置:** `handlers.rs:461-468`

**现状:**
```rust
fn extract_param_from_url(url: &str, param: &str) -> Option<String> {
    url.split('?')
        .nth(1)?
        .split('&')
        .find(|p| p.starts_with(&format!("{}=", param)))
        .and_then(|p| p.split('=').nth(1))
        .map(|v| v.to_string())
}
```

**crate 提供的替代方案:**
```rust
use pixiv_client::models::common::parse_next_url;

if let Some(params) = parse_next_url(&next_url) {
    max_bookmark_id = params.get("max_bookmark_id").and_then(|v| v.parse().ok());
}
```

**问题:** 自定义实现没有 URL 解码（`%XX`），crate 的实现使用 `url::Url::parse()` 更健壮。

**风险:** 极低。Pixiv 的 `max_bookmark_id` 是纯数字，不需要 URL 解码。

---

### D3: 未使用 resolve_download_tasks ⚠️ 建议改进

**位置:** `handlers.rs:422-459` (`get_image_pages`)

**现状:** 手动实现图片 URL 解析逻辑：
```rust
fn get_image_pages(illust: &Illust) -> Vec<(String, String)> {
    // 手动检查 meta_pages → meta_single_page → image_urls
    // 手动实现 original > large > medium 回退链
}
```

**crate 提供的替代方案:**
```rust
use pixiv_client::downloader::resolve_download_tasks;

let tasks = resolve_download_tasks(illust, "original", None);
// tasks 包含所有页面的 url 和 filename
```

**差异:**
- 后端的 `get_image_pages` 优先使用 `original`，然后 `large`，然后 `medium` — 与 crate 的 `url_for_size("original")` 逻辑一致
- 后端生成的文件名格式为 `{illust_id}_p{page_idx}.{ext}`，crate 生成的格式不同
- 后端根据 URL 是否包含 `.png` 判断扩展名，crate 使用 `extract_ext()` 更健壮

**风险:** 低。当前实现逻辑正确，只是代码重复。

---

### D4: handle_refresh_pixiv_token 不调用 touch_last_used ℹ️ 信息

**位置:** `handlers.rs:838-893`

**现状:** `handle_refresh_pixiv_token` 直接调用 `api.auth()` 而不通过 `auth_with_credential()`。

**差异:**
- `auth_with_credential()` 会调用 `touch_last_used()` 更新 `last_used_at`
- `handle_refresh_pixiv_token` 不会更新 `last_used_at`

**影响:** 刷新 token 时不会更新凭证的使用时间戳。由于 `last_used_at` 目前仅用于记录，不影响功能。

**建议:** 如果需要追踪凭证使用频率，在 `handle_refresh_pixiv_token` 成功后也调用 `touch_last_used()`。

---

### D5: Auth 恢复只重试一次 ✅ 可接受

**位置:** handlers.rs:107-113, 180-185, 251-255, 785-789

**模式:**
```rust
Err(e) if e.is_auth_error() => {
    recover_auth(api, credential_id, &state.db).await?;
    api.method().await.map_err(|e| format!("... after auth recovery: {}", e))?
}
```

**分析:** 只重试一次是合理的。如果恢复后仍然 401，说明 refresh_token 本身已失效，重试更多次无意义。此时应让任务失败，触发凭证状态更新或人工介入。

**结论:** ✅ 正确行为，无需修改。

---

### D6: crawl_bookmarks 硬编码 "public" ℹ️ 信息

**位置:** `handlers.rs:247`

```rust
api.user_bookmarks_illust(user_id, Some("public"), max_bookmark_id, tags)
```

**分析:** 只爬取公开书签，不支持私密书签。这是合理的设计选择——私密书签需要特殊权限。

**如果需要支持:** 可以在 `CrawlJob` 中添加 `bookmark_restrict` 字段，从 API 端点传入。

---

### D7: save_illust 中的 unwrap() ⚠️ 轻微

**位置:** `handlers.rs:633`

```rust
active.colors = Set(Some(serde_json::to_value(&colors).unwrap()));
```

**分析:** `colors` 是 `ThemeColors` 结构体，实现了 `Serialize`。对于已知类型，`serde_json::to_value` 不会失败。但如果结构体未来添加了自定义序列化逻辑，可能会 panic。

**建议:** 替换为 `.map_err(|e| format!("Failed to serialize colors: {}", e))?`。

---

### D8: 无速率限制 ⚠️ 需要关注

**现状:** crate 没有内置速率限制，后端也没有添加。

**风险:** 如果爬取速度过快，Pixiv 可能返回 429 (Too Many Requests) 或临时封禁 IP。

**当前缓解措施:**
- `max_pages` 参数限制每次爬取的页数
- worker 并发数有限（crawl=2, discover=1）
- 任务之间有自然间隔（任务队列调度）

**建议:** 监控 429 错误的发生频率。如果频繁出现，在 crawl 循环中添加延迟：
```rust
tokio::time::sleep(Duration::from_millis(500)).await; // 每页间隔 500ms
```

---

### D9: discover 使用当前 ID 作为 root_job_id ℹ️ 信息

**位置:** `handlers.rs:800-805`

```rust
storage.push_download_with_parent(DownloadJob {
    // ...
    parent_job_id: Some(current_id.clone()),
    root_job_id: Some(current_id.clone()),  // discover 任务 ID 作为 root
}, &state.db).await
```

**分析:** crawl 任务使用 crawl 任务 ID 作为 `root_job_id`，使下游任务成为 crawl 的直接子任务。discover 任务也使用自己的 ID 作为 `root_job_id`，这意味着 discover 发现的下载任务是 discover 的子任务，而不是原始 crawl 的子任务。

**影响:** 任务树结构变为：
```
Crawl
├── Download 1 (root = Crawl)
├── Download 2 (root = Crawl)
└── Discover (root = Crawl)
    ├── Download 3 (root = Discover)  ← 不是 Crawl 的直接子任务
    └── Download 4 (root = Discover)
```

**这可能是有意设计** — discover 有自己的任务树层级。但如果希望所有下载都归到原始 crawl 下，需要传递原始 crawl 的 ID。

---

### D10: 已修复 ✅ — downloader 模块已被使用

**位置:** `src/pixiv/mod.rs:4-6`

```rust
pub mod downloader {
    pub use pixiv_client::downloader::*;
}
```

**现状:** `handle_download` 现在使用 `crate::pixiv::downloader::DownloadManager`。

---

## 10. 改进建议

### 优先级高

| # | 建议 | 影响 | 工作量 |
|---|------|------|--------|
| 1 | 在 crawl 循环中添加页面间延迟（500ms-1s） | 避免触发 Pixiv 限流 | 小 |
| 2 | 监控 `PixivError::Status(429)` 并记录告警 | 及时发现限流问题 | 小 |

### 优先级中

| # | 建议 | 影响 | 工作量 |
|---|------|------|--------|
| 3 | 评估使用 DownloadManager 替换手动下载 | 获得重试能力，减少下载失败 | 中 |
| 4 | 使用 `parse_next_url` 替换 `extract_param_from_url` | 更健壮的 URL 解析 | 小 |
| 5 | 使用 `resolve_download_tasks` 替换 `get_image_pages` | 减少代码重复 | 中 |
| 6 | 移除未使用的 `downloader` re-export | 减少代码噪音 | 极小 |

### 优先级低

| # | 建议 | 影响 | 工作量 |
|---|------|------|--------|
| 7 | 在 `handle_refresh_pixiv_token` 中添加 `touch_last_used` | 完整的凭证使用追踪 | 极小 |
| 8 | 将 `serde_json::to_value().unwrap()` 替换为错误处理 | 防御性编程 | 极小 |
| 9 | 考虑启用 `gfw-bypass` feature（如果服务器在国内） | 绕过 GFW 直连 Pixiv | 极小 |

---

## 附录：后端使用的 API 方法汇总

| API 方法 | 调用位置 | 用途 |
|----------|----------|------|
| `PixivApi::new()` | pixiv/mod.rs:22 | 创建默认客户端 |
| `PixivApi::with_config()` | pixiv/mod.rs:24-28 | 创建带代理的客户端 |
| `set_accept_lang()` | pixiv/mod.rs:31 | 设置 Accept-Language |
| `auth()` | pixiv/mod.rs:52, 82; handlers.rs:858 | OAuth token 交换 |
| `set_auth()` | pixiv/mod.rs:50 | 设置已有的 token |
| `access_token()` | pixiv/mod.rs:96; handlers.rs:865 | 读取 access_token |
| `current_refresh_token()` | pixiv/mod.rs:95; handlers.rs:864 | 读取 refresh_token |
| `user_id()` | handlers.rs:236 | 获取当前用户 ID |
| `illust_ranking()` | handlers.rs:103, 109 | 爬取排行榜 |
| `user_illusts()` | handlers.rs:176, 182 | 爬取用户作品 |
| `user_bookmarks_illust()` | handlers.rs:247, 253 | 爬取用户书签 |
| `illust_related()` | handlers.rs:783, 787 | 发现相关作品 |

---

*文档生成时间: 2026-06-02*
*pixiv-client 版本: 1.2.0*
