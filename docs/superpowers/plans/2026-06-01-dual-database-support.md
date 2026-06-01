# Dual Database Support (SQLite + PostgreSQL) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the entire backend run on both SQLite (development) and PostgreSQL (production) with minimal `cfg` duplication — all backend-specific code isolated into a single module.

**Architecture:** Create `src/db_backend.rs` as the **sole** module that knows which database backend is active. It exposes type aliases (`ApalisPool`, `JobStorage`) and an init function. All other code uses `apalis::prelude::TaskSink` trait generics for push operations — written once, works for both. Migrations use `cfg` blocks (unavoidable — SQL dialects differ). The task listing handler uses `cfg` (unavoidable — raw SQL schema differs).

**Tech Stack:** `apalis-sqlite` / `apalis-postgres` (1.0.0-rc.8), `sqlx` 0.8, `sea-orm` 1, `sea-orm-migration` 1

---

## Key Design Decision

**Why not a unified trait?** `apalis::Backend` is NOT object-safe (`poll` consumes `self`, associated `Stream`/`Beat` types, RPITIT). No `Box<dyn Backend>`. No shared storage type alias exists.

**What works:**
- `TaskSink<Args>` — blanket impl on both backends. `push()` is the only method `JobStorage` needs.
- `WorkerBuilder::backend()` — fully generic on `NB: Backend`. Works for both.
- Type aliases with `cfg` — one module, two branches, rest of code sees one type.

**Result:** `cfg` appears in exactly 3 places:
1. `src/db_backend.rs` — type aliases + init (5 `cfg` blocks)
2. `src/handlers/task.rs` — raw SQL queries (1 `cfg` block per query)
3. `migration/src/` — dialect-specific SQL (2 migration files)

---

## File Map

| File | Action | `cfg` count | Why |
|---|---|---|---|
| `Cargo.toml` | Modify | 0 | Feature flags only |
| `migration/Cargo.toml` | Modify | 0 | Feature flags only |
| **`src/db_backend.rs`** | **Create** | **~5** | **All backend-specific types + init** |
| `src/lib.rs` | Modify | 2 | Import from `db_backend`, define `ApalisPool` type |
| `src/main.rs` | Modify | 0 | Call `db_backend::init()`, no cfg needed |
| `src/task_queue/handlers.rs` | Modify | 0 | Use trait generics, no cfg needed |
| `src/handlers/task.rs` | Modify | ~5 | Raw SQL dialect differences |
| `migration/src/m20260531_000002_schema_refactor.rs` | Modify | ~8 | `json_extract` vs `->>`, column types |
| `migration/src/m20260531_000005_add_deleted_at.rs` | Modify | 1 | `DATETIME` vs `TIMESTAMPTZ` |
| Other migration files | No change | 0 | Already portable |

---

## Task 1: Cargo.toml — Feature flags

**Files:**
- Modify: `Cargo.toml`
- Modify: `migration/Cargo.toml`

- [ ] **Step 1: Add feature flags to root `Cargo.toml`**

Add a `[features]` section. `sqlite` is the default. Gate `apalis-sqlite` and `apalis-postgres` as optional deps. Gate `sqlx` backend feature.

```toml
[features]
default = ["sqlite"]
sqlite = ["dep:apalis-sqlite", "sqlx/sqlite"]
postgres = ["dep:apalis-postgres", "sqlx/postgres"]

[dependencies]
# ... keep all existing deps ...

# Background job queue — gated backends
apalis-sqlite = { version = "1.0.0-rc.8", default-features = false, features = ["tokio-comp", "migrate", "json", "chrono"], optional = true }
apalis-postgres = { version = "1.0.0-rc.8", default-features = false, features = ["tokio-comp", "migrate", "chrono"], optional = true }

# Common apalis deps (always included)
apalis = { version = "1.0.0-rc.9", features = ["retry", "tracing"] }
apalis-codec = { version = "0.1.0-rc.9", features = ["json"] }

# Direct sqlx — backend feature gated above
sqlx = { version = "0.8", features = ["runtime-tokio-rustls"] }
```

Remove the old non-optional `apalis-sqlite` and `sqlx` (with hardcoded `"sqlite"`) lines.

- [ ] **Step 2: Gate migration crate features**

```toml
# migration/Cargo.toml
[features]
default = ["sqlite"]
sqlite = ["sea-orm-migration/sqlx-sqlite"]
postgres = ["sea-orm-migration/sqlx-postgres"]

[dependencies]
sea-orm-migration = { version = "1", features = ["runtime-tokio-rustls"] }
```

- [ ] **Step 3: Verify both features compile**

```bash
cargo check --features sqlite
cargo check --features postgres
```

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml migration/Cargo.toml
git commit -m "build: add sqlite/postgres feature flags for dual-database support"
```

---

## Task 2: Create `src/db_backend.rs` — the single backend abstraction module

**Files:**
- Create: `src/db_backend.rs`

This is the **only** file in the application that contains `#[cfg(feature = "...")]` for database backend selection. Everything else uses the types exported from here.

- [ ] **Step 1: Create the module**

```rust
//! Database backend abstraction.
//!
//! This is the ONLY module that compiles differently for sqlite vs postgres.
//! All other code uses the type aliases and functions exported from here.

use apalis::prelude::*;

// ---------------------------------------------------------------------------
// Pool type
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlite")]
pub type Pool = apalis_sqlite::SqlitePool;

#[cfg(feature = "postgres")]
pub type Pool = apalis_postgres::PgPool;

// ---------------------------------------------------------------------------
// Storage type (for a single job type T)
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlite")]
type Storage<T> = apalis_sqlite::SqliteStorage<
    T,
    apalis_codec::json::JsonCodec<Vec<u8>>,
    apalis_sqlite::fetcher::SqliteFetcher,
>;

#[cfg(feature = "postgres")]
type Storage<T> = apalis_postgres::PostgresStorage<T>;

// ---------------------------------------------------------------------------
// JobStorage — holds all 7 typed queues, push via TaskSink trait
// ---------------------------------------------------------------------------

/// Holds all typed job storages. Each storage is mutex-wrapped
/// because `TaskSink::push` requires `&mut self`.
///
/// Push operations use the `TaskSink` trait — no backend-specific code needed.
#[derive(Clone)]
pub struct JobStorage {
    pub crawl: std::sync::Arc<tokio::sync::Mutex<Storage<CrawlJob>>>,
    pub download: std::sync::Arc<tokio::sync::Mutex<Storage<DownloadJob>>>,
    pub color_extract: std::sync::Arc<tokio::sync::Mutex<Storage<ColorExtractJob>>>,
    pub upload: std::sync::Arc<tokio::sync::Mutex<Storage<UploadJob>>>,
    pub accessibility_check: std::sync::Arc<tokio::sync::Mutex<Storage<AccessibilityCheckJob>>>,
    pub discover: std::sync::Arc<tokio::sync::Mutex<Storage<DiscoverJob>>>,
    pub refresh_pixiv_token: std::sync::Arc<tokio::sync::Mutex<Storage<RefreshPixivTokenJob>>>,
}

// Import job types
use crate::task_queue::jobs::*;

impl JobStorage {
    fn new_storage<T>(pool: &Pool) -> Storage<T> {
        #[cfg(feature = "sqlite")]
        { apalis_sqlite::SqliteStorage::new(pool) }
        #[cfg(feature = "postgres")]
        { apalis_postgres::PostgresStorage::new(pool) }
    }

    pub fn new(pool: &Pool) -> Self {
        use std::sync::Arc;
        use tokio::sync::Mutex;
        Self {
            crawl: Arc::new(Mutex::new(Self::new_storage(pool))),
            download: Arc::new(Mutex::new(Self::new_storage(pool))),
            color_extract: Arc::new(Mutex::new(Self::new_storage(pool))),
            upload: Arc::new(Mutex::new(Self::new_storage(pool))),
            accessibility_check: Arc::new(Mutex::new(Self::new_storage(pool))),
            discover: Arc::new(Mutex::new(Self::new_storage(pool))),
            refresh_pixiv_token: Arc::new(Mutex::new(Self::new_storage(pool))),
        }
    }
}

// ---------------------------------------------------------------------------
// Push methods — generic via TaskSink, NO cfg needed
// ---------------------------------------------------------------------------

impl JobStorage {
    pub async fn push_crawl(&self, job: CrawlJob) -> Result<(), String> {
        self.crawl.lock().await.push(job).await.map_err(|e| e.to_string())
    }
    pub async fn push_download(&self, job: DownloadJob) -> Result<(), String> {
        self.download.lock().await.push(job).await.map_err(|e| e.to_string())
    }
    pub async fn push_color_extract(&self, job: ColorExtractJob) -> Result<(), String> {
        self.color_extract.lock().await.push(job).await.map_err(|e| e.to_string())
    }
    pub async fn push_upload(&self, job: UploadJob) -> Result<(), String> {
        self.upload.lock().await.push(job).await.map_err(|e| e.to_string())
    }
    pub async fn push_accessibility_check(&self, job: AccessibilityCheckJob) -> Result<(), String> {
        self.accessibility_check.lock().await.push(job).await.map_err(|e| e.to_string())
    }
    pub async fn push_discover(&self, job: DiscoverJob) -> Result<(), String> {
        self.discover.lock().await.push(job).await.map_err(|e| e.to_string())
    }
    pub async fn push_refresh_pixiv_token(&self, job: RefreshPixivTokenJob) -> Result<(), String> {
        self.refresh_pixiv_token.lock().await.push(job).await.map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Pool init + Apalis setup
// ---------------------------------------------------------------------------

/// Connect to the database and run Apalis internal migrations.
/// Returns (pool, job_storage).
pub async fn init(database_url: &str) -> (Pool, JobStorage) {
    #[cfg(feature = "sqlite")]
    {
        let pool = apalis_sqlite::SqlitePool::connect(database_url)
            .await
            .expect("Failed to connect Apalis SQLite pool");
        apalis_sqlite::SqliteStorage::<(), (), ()>::setup(&pool)
            .await
            .expect("Failed to run Apalis SQLite migrations");
        let js = JobStorage::new(&pool);
        (pool, js)
    }
    #[cfg(feature = "postgres")]
    {
        let pool = apalis_postgres::PgPool::connect(database_url)
            .await
            .expect("Failed to connect Apalis PostgreSQL pool");
        apalis_postgres::PostgresStorage::<()>::setup(&pool)
            .await
            .expect("Failed to run Apalis PostgreSQL migrations");
        let js = JobStorage::new(&pool);
        (pool, js)
    }
}
```

- [ ] **Step 2: Register the module in `src/lib.rs`**

```rust
pub mod db_backend;  // add this line
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check --features sqlite
cargo check --features postgres
```

- [ ] **Step 4: Commit**

```bash
git add src/db_backend.rs src/lib.rs
git commit -m "feat: add db_backend module — single point of backend abstraction"
```

---

## Task 3: Rewrite `src/lib.rs` — use `db_backend` types

**Files:**
- Modify: `src/lib.rs:1-20`

- [ ] **Step 1: Replace inline types with imports from `db_backend`**

```rust
pub mod auth;
pub mod color;
pub mod config;
pub mod db;
pub mod db_backend;
pub mod dogecloud;
pub mod error;
pub mod handlers;
pub mod pixiv;
pub mod task_queue;

use config::AppConfig;

/// The Apalis connection pool type — determined by feature flag.
pub type ApalisPool = db_backend::Pool;

#[derive(Clone)]
pub struct AppState {
    pub db: sea_orm::DatabaseConnection,
    pub config: AppConfig,
    pub oss: dogecloud::DogeCloudOss,
    pub job_storage: db_backend::JobStorage,
    pub apalis_pool: ApalisPool,
}
```

- [ ] **Step 2: Verify compilation**

```bash
cargo check --features sqlite
cargo check --features postgres
```

- [ ] **Step 3: Commit**

```bash
git add src/lib.rs
git commit -m "refactor: use db_backend types in AppState"
```

---

## Task 4: Simplify `src/main.rs` — call `db_backend::init()`

**Files:**
- Modify: `src/main.rs:1-268`

- [ ] **Step 1: Replace Apalis init block (lines 68-76) with `db_backend::init()`**

Old code:
```rust
let apalis_pool = apalis_sqlite::SqlitePool::connect(&config.database_url)
    .await
    .expect("Failed to connect Apalis SQLite pool");
SqliteStorage::<(), (), ()>::setup(&apalis_pool)
    .await
    .expect("Failed to run Apalis migrations");
let job_storage = JobStorage::new(&apalis_pool);
```

New code:
```rust
let (apalis_pool, job_storage) = db_backend::init(&config.database_url).await;
```

- [ ] **Step 2: Update `spawn_workers` signature**

Change `_pool: &apalis_sqlite::SqlitePool` to `_pool: &ApalisPool`:

```rust
fn spawn_workers(
    state: Arc<AppState>,
    _pool: &ApalisPool,
) -> Vec<tokio::task::JoinHandle<()>> {
```

- [ ] **Step 3: Remove unused imports**

Remove `use apalis_sqlite::SqliteStorage;` from line 3. Remove `use randimg_backend_rs::task_queue::handlers::JobStorage;` if `JobStorage` now comes from `db_backend`.

Update import:
```rust
use randimg_backend_rs::task_queue::handlers::*;  // keep handler functions
// JobStorage is now in AppState, no direct import needed
```

- [ ] **Step 4: Update `spawn_worker!` macro**

The macro accesses `js.crawl`, `js.download`, etc. These fields are the same on `db_backend::JobStorage` as on the old struct. No change needed to the macro body — it uses `.blocking_lock().clone()` which works for any `Arc<Mutex<T>>` where `T: Clone`.

- [ ] **Step 5: Verify compilation**

```bash
cargo check --features sqlite
cargo check --features postgres
```

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "refactor: simplify main.rs with db_backend::init()"
```

---

## Task 5: Delete old `src/task_queue/handlers.rs` JobStorage + push methods

**Files:**
- Modify: `src/task_queue/handlers.rs:1-116`

- [ ] **Step 1: Remove the `JobStorage` struct, `new()`, and all `push_*` methods**

Lines 1-116 of `handlers.rs` contain the old `JobStorage` type alias, struct, and push methods. Delete all of this — it now lives in `db_backend.rs`.

Keep the handler functions (`handle_crawl`, `handle_download`, etc.) starting from line 118. They only use `JobStorage` through `Data<JobStorage>` which resolves to `db_backend::JobStorage` via `AppState`.

- [ ] **Step 2: Update imports at the top of the file**

```rust
use std::sync::Arc;

use apalis::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};

use crate::AppState;
use crate::db::query;
use crate::db::query::image::SeedMethod;
use crate::pixiv::PixivApi;

use super::jobs::*;
```

Remove `use apalis_sqlite::SqliteStorage;`, `use tokio::sync::Mutex;`, and the `JsonSqliteStorage` type alias.

- [ ] **Step 3: Verify compilation**

```bash
cargo check --features sqlite
cargo check --features postgres
```

- [ ] **Step 4: Commit**

```bash
git add src/task_queue/handlers.rs
git commit -m "refactor: move JobStorage to db_backend, keep only handlers"
```

---

## Task 6: Feature-gate `handlers/task.rs` — raw SQL queries

**Files:**
- Modify: `src/handlers/task.rs:1-193`

This is the one remaining place outside `db_backend.rs` that needs `cfg`. The raw SQL queries hit the Apalis `Jobs` table directly — table name, parameter syntax, and column types differ between backends.

**Verified schema differences** (from reading `apalis-postgres-1.0.0-rc.8/migrations/`):

| | SQLite | PostgreSQL |
|---|---|---|
| Table | `Jobs` | `apalis.jobs` |
| `run_at` | `INTEGER` (unix seconds) | `TIMESTAMPTZ` |
| `done_at` | `INTEGER` (unix seconds) | `TIMESTAMPTZ` |
| `last_result` | `TEXT` | `JSONB` |
| Params | `?1, ?2` | `$1, $2` |

- [ ] **Step 1: Replace the entire file**

```rust
use axum::{
    Json, Router,
    extract::Path,
    extract::Query,
    extract::State,
    routing::get,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks", get(list_tasks))
        .route("/tasks/{task_id}", get(get_task))
}

#[derive(Deserialize)]
pub struct ListTasksQuery {
    pub task_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

fn map_status(status: &str) -> &str {
    match status {
        "Pending" => "pending",
        "Running" => "running",
        "Done" => "completed",
        "Failed" => "failed",
        "Killed" => "killed",
        other => other,
    }
}

fn unmap_status(status: &str) -> &str {
    match status {
        "pending" => "Pending",
        "running" => "Running",
        "completed" => "Done",
        "failed" => "Failed",
        "killed" => "Killed",
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Row type — feature-gated field types for schema differences
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ApalisJobRow {
    id: String,
    job_type: String,
    status: String,
    attempts: i32,
    max_attempts: i32,
    #[cfg(feature = "sqlite")]
    run_at: i64,
    #[cfg(feature = "postgres")]
    run_at: chrono::DateTime<chrono::Utc>,
    #[cfg(feature = "sqlite")]
    done_at: Option<i64>,
    #[cfg(feature = "postgres")]
    done_at: Option<chrono::DateTime<chrono::Utc>>,
    #[cfg(feature = "sqlite")]
    last_result: Option<String>,
    #[cfg(feature = "postgres")]
    last_result: Option<serde_json::Value>,
    priority: i32,
}

// ---------------------------------------------------------------------------
// Timestamp formatting — feature-gated for type differences
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlite")]
fn fmt_ts(ts: Option<i64>) -> Option<String> {
    ts.and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

#[cfg(feature = "postgres")]
fn fmt_ts(ts: Option<chrono::DateTime<chrono::Utc>>) -> Option<String> {
    ts.map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

#[cfg(feature = "sqlite")]
fn fmt_ts_req(ts: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

#[cfg(feature = "postgres")]
fn fmt_ts_req(ts: chrono::DateTime<chrono::Utc>) -> Option<String> {
    Some(ts.format("%Y-%m-%d %H:%M:%S").to_string())
}

#[cfg(feature = "sqlite")]
fn fmt_last_result(v: &Option<String>) -> Option<String> {
    v.clone()
}

#[cfg(feature = "postgres")]
fn fmt_last_result(v: &Option<serde_json::Value>) -> Option<String> {
    v.as_ref().map(|v| v.to_string())
}

// ---------------------------------------------------------------------------
// SQL helpers — feature-gated for parameter syntax and table name
// ---------------------------------------------------------------------------

const SELECT_COLS: &str = "id, job_type, status, attempts, max_attempts, run_at, done_at, last_result, priority";

#[cfg(feature = "sqlite")]
const JOBS_TABLE: &str = "Jobs";

#[cfg(feature = "postgres")]
const JOBS_TABLE: &str = "apalis.jobs";

/// Bind parameters and fetch all rows. Uses ?N for SQLite, $N for Postgres.
async fn fetch_tasks(
    pool: &crate::ApalisPool,
    task_type: Option<&str>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<ApalisJobRow>, sqlx::Error> {
    match (task_type, status) {
        (Some(tt), Some(st)) => {
            #[cfg(feature = "sqlite")]
            { sqlx::query_as::<_, ApalisJobRow>(
                &format!("{} FROM {} WHERE job_type = ?1 AND status = ?2 ORDER BY run_at DESC LIMIT ?3 OFFSET ?4", SELECT_COLS, JOBS_TABLE)
            ).bind(tt).bind(st).bind(limit).bind(offset).fetch_all(pool).await }
            #[cfg(feature = "postgres")]
            { sqlx::query_as::<_, ApalisJobRow>(
                &format!("{} FROM {} WHERE job_type = $1 AND status = $2 ORDER BY run_at DESC LIMIT $3 OFFSET $4", SELECT_COLS, JOBS_TABLE)
            ).bind(tt).bind(st).bind(limit).bind(offset).fetch_all(pool).await }
        }
        (Some(tt), None) => {
            #[cfg(feature = "sqlite")]
            { sqlx::query_as::<_, ApalisJobRow>(
                &format!("{} FROM {} WHERE job_type = ?1 ORDER BY run_at DESC LIMIT ?2 OFFSET ?3", SELECT_COLS, JOBS_TABLE)
            ).bind(tt).bind(limit).bind(offset).fetch_all(pool).await }
            #[cfg(feature = "postgres")]
            { sqlx::query_as::<_, ApalisJobRow>(
                &format!("{} FROM {} WHERE job_type = $1 ORDER BY run_at DESC LIMIT $2 OFFSET $3", SELECT_COLS, JOBS_TABLE)
            ).bind(tt).bind(limit).bind(offset).fetch_all(pool).await }
        }
        (None, Some(st)) => {
            #[cfg(feature = "sqlite")]
            { sqlx::query_as::<_, ApalisJobRow>(
                &format!("{} FROM {} WHERE status = ?1 ORDER BY run_at DESC LIMIT ?2 OFFSET ?3", SELECT_COLS, JOBS_TABLE)
            ).bind(st).bind(limit).bind(offset).fetch_all(pool).await }
            #[cfg(feature = "postgres")]
            { sqlx::query_as::<_, ApalisJobRow>(
                &format!("{} FROM {} WHERE status = $1 ORDER BY run_at DESC LIMIT $2 OFFSET $3", SELECT_COLS, JOBS_TABLE)
            ).bind(st).bind(limit).bind(offset).fetch_all(pool).await }
        }
        (None, None) => {
            #[cfg(feature = "sqlite")]
            { sqlx::query_as::<_, ApalisJobRow>(
                &format!("{} FROM {} ORDER BY run_at DESC LIMIT ?1 OFFSET ?2", SELECT_COLS, JOBS_TABLE)
            ).bind(limit).bind(offset).fetch_all(pool).await }
            #[cfg(feature = "postgres")]
            { sqlx::query_as::<_, ApalisJobRow>(
                &format!("{} FROM {} ORDER BY run_at DESC LIMIT $1 OFFSET $2", SELECT_COLS, JOBS_TABLE)
            ).bind(limit).bind(offset).fetch_all(pool).await }
        }
    }
}

fn row_to_json(t: &ApalisJobRow) -> serde_json::Value {
    serde_json::json!({
        "id": t.id,
        "task_type": t.job_type,
        "status": map_status(&t.status),
        "priority": t.priority,
        "retry_count": t.attempts,
        "max_retries": t.max_attempts,
        "created_at": fmt_ts_req(t.run_at),
        "started_at": serde_json::Value::Null,
        "finished_at": fmt_ts(t.done_at),
        "last_error": fmt_last_result(&t.last_result),
    })
}

// ---------------------------------------------------------------------------
// Handlers — NO cfg needed, all backend differences are in helpers above
// ---------------------------------------------------------------------------

pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<ListTasksQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let limit = q.limit.unwrap_or(50).min(200) as i64;
    let offset = q.offset.unwrap_or(0) as i64;

    let rows = fetch_tasks(
        &state.apalis_pool,
        q.task_type.as_deref(),
        q.status.as_deref().map(unmap_status),
        limit,
        offset,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(rows.iter().map(row_to_json).collect()))
}

pub async fn get_task(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = &state.apalis_pool;

    #[cfg(feature = "sqlite")]
    let row = sqlx::query_as::<_, ApalisJobRow>(
        &format!("{} FROM {} WHERE id = ?1", SELECT_COLS, JOBS_TABLE)
    )
    .bind(&task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    #[cfg(feature = "postgres")]
    let row = sqlx::query_as::<_, ApalisJobRow>(
        &format!("{} FROM {} WHERE id = $1", SELECT_COLS, JOBS_TABLE)
    )
    .bind(&task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    match row {
        Some(t) => Ok(Json(row_to_json(&t))),
        None => Err(AppError::NotFound(format!("Task {} not found", task_id))),
    }
}
```

- [ ] **Step 2: Verify compilation**

```bash
cargo check --features sqlite
cargo check --features postgres
```

- [ ] **Step 3: Commit**

```bash
git add src/handlers/task.rs
git commit -m "refactor: feature-gate task handler with isolated cfg blocks"
```

---

## Task 7: Feature-gate migration 002 (schema refactor)

**Files:**
- Modify: `migration/src/m20260531_000002_schema_refactor.rs`

- [ ] **Step 1: Add `cfg` blocks for `json_extract` vs `->>`**

Replace lines 20-30 (the `background_tasks` JSON backfill):

```rust
        // Backfill background_tasks.image_id from payload JSON
        #[cfg(feature = "sqlite")]
        db.execute_unprepared(
            "UPDATE background_tasks SET image_id = CAST(json_extract(payload, '$.image_id') AS INTEGER)
             WHERE json_extract(payload, '$.image_id') IS NOT NULL"
        ).await?;

        #[cfg(feature = "postgres")]
        db.execute_unprepared(
            "UPDATE background_tasks SET image_id = (payload::json->>'image_id')::INTEGER
             WHERE payload::json->>'image_id' IS NOT NULL"
        ).await?;

        // Backfill background_tasks.image_path from payload JSON
        #[cfg(feature = "sqlite")]
        db.execute_unprepared(
            "UPDATE background_tasks SET image_path = json_extract(payload, '$.image_path')
             WHERE json_extract(payload, '$.image_path') IS NOT NULL"
        ).await?;

        #[cfg(feature = "postgres")]
        db.execute_unprepared(
            "UPDATE background_tasks SET image_path = payload::json->>'image_path'
             WHERE payload::json->>'image_path' IS NOT NULL"
        ).await?;
```

- [ ] **Step 2: Feature-gate column types for `ALTER TABLE ADD COLUMN`**

Replace lines 33-63:

```rust
        // ========== 2. images new business fields ==========
        #[cfg(feature = "sqlite")]
        {
            db.execute_unprepared("ALTER TABLE images ADD COLUMN is_public BOOLEAN NOT NULL DEFAULT 0").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN avatar_available BOOLEAN").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN source_created_at DATETIME").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN total_view BIGINT NOT NULL DEFAULT 0").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN total_bookmarks BIGINT NOT NULL DEFAULT 0").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN total_comments BIGINT NOT NULL DEFAULT 0").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN fetched_times INTEGER NOT NULL DEFAULT 0").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP").await?;
        }

        #[cfg(feature = "postgres")]
        {
            db.execute_unprepared("ALTER TABLE images ADD COLUMN is_public BOOLEAN NOT NULL DEFAULT false").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN avatar_available BOOLEAN").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN source_created_at TIMESTAMPTZ").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN total_view BIGINT NOT NULL DEFAULT 0").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN total_bookmarks BIGINT NOT NULL DEFAULT 0").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN total_comments BIGINT NOT NULL DEFAULT 0").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN fetched_times INTEGER NOT NULL DEFAULT 0").await?;
            db.execute_unprepared("ALTER TABLE images ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()").await?;
        }
```

- [ ] **Step 3: Feature-gate the `is_public` data migration**

Replace lines 68-72. Note: column is still `accessable` at this point (renamed in migration 004).

```rust
        // ========== 3. Data migration: compute is_public ==========
        #[cfg(feature = "sqlite")]
        db.execute_unprepared(
            "UPDATE images SET is_public = CASE
                WHEN uploaded = 1 AND processed = 1 AND (accessable IS NULL OR accessable = 1)
                THEN 1 ELSE 0 END"
        ).await?;

        #[cfg(feature = "postgres")]
        db.execute_unprepared(
            "UPDATE images SET is_public = CASE
                WHEN uploaded = true AND processed = true AND (accessable IS NULL OR accessable = true)
                THEN true ELSE false END"
        ).await?;
```

- [ ] **Step 4: Feature-gate the `down` migration**

The `down` method uses `BOOLEAN NOT NULL DEFAULT 0` (SQLite) vs `DEFAULT false` (Postgres). Wrap the `ADD COLUMN` restores in cfg blocks. The `DROP COLUMN` statements work on both — keep shared.

- [ ] **Step 5: Commit**

```bash
git add migration/src/m20260531_000002_schema_refactor.rs
git commit -m "feat(migration): feature-gate migration 002 for sqlite/postgres"
```

---

## Task 8: Feature-gate migration 005 (add deleted_at)

**Files:**
- Modify: `migration/src/m20260531_000005_add_deleted_at.rs`

- [ ] **Step 1: Feature-gate `DATETIME` vs `TIMESTAMPTZ`**

```rust
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        #[cfg(feature = "sqlite")]
        db.execute_unprepared("ALTER TABLE images ADD COLUMN deleted_at DATETIME").await?;
        #[cfg(feature = "postgres")]
        db.execute_unprepared("ALTER TABLE images ADD COLUMN deleted_at TIMESTAMPTZ").await?;
        Ok(())
    }
```

The `down` method's `DROP COLUMN` works on both — no change needed.

- [ ] **Step 2: Commit**

```bash
git add migration/src/m20260531_000005_add_deleted_at.rs
git commit -m "feat(migration): feature-gate migration 005 for sqlite/postgres"
```

---

## Task 9: Clean up `background_tasks` remnant

**Files:**
- Modify: `migration/src/m20260531_000001_create_base_tables.rs`
- Modify: `migration/src/m20260531_000002_schema_refactor.rs`

- [ ] **Step 1: Remove `background_tasks` table creation from migration 001**

Find the `BackgroundTasks` table creation block and the `BackgroundTasks` enum. Remove both.

- [ ] **Step 2: Remove `background_tasks` ALTER/UPDATE from migration 002**

Remove the `ADD COLUMN image_id`, `ADD COLUMN image_path`, and the two `UPDATE` backfill statements (the ones we just cfg-gated in Task 7 — remove them entirely since the table won't exist). Also remove the corresponding `DROP COLUMN` from the `down` method.

- [ ] **Step 3: Verify compilation**

```bash
cargo check --features sqlite
```

- [ ] **Step 4: Commit**

```bash
git add migration/src/
git commit -m "chore(migration): remove unused background_tasks table"
```

---

## Task 10: End-to-end smoke test

- [ ] **Step 1: Build and run with SQLite (default)**

```bash
cargo build
cargo run
```

Verify server starts, workers spawn, `/tasks` endpoint responds.

- [ ] **Step 2: Build with PostgreSQL feature**

```bash
cargo build --features postgres --no-default-features
```

Compilation-only verification. Running requires a PostgreSQL instance.

- [ ] **Step 3: Run existing tests**

```bash
cargo test
```

- [ ] **Step 4: Commit**

```bash
git commit -m "chore: verify dual-database build"
```

---

## Where `cfg` appears (summary)

| Location | Count | What |
|---|---|---|
| `src/db_backend.rs` | ~5 | Pool type, Storage type, `new_storage()`, `init()` |
| `src/handlers/task.rs` | ~8 | `ApalisJobRow` fields, `fmt_ts`, `fmt_last_result`, `fetch_tasks` SQL |
| `migration/src/m*_000002_*` | ~8 | `json_extract`/`->>`, column types, boolean literals |
| `migration/src/m*_000005_*` | 1 | `DATETIME`/`TIMESTAMPTZ` |
| `src/lib.rs` | 1 | `pub type ApalisPool` |
| **Everywhere else** | **0** | `main.rs`, `handlers/*.rs`, `task_queue/*.rs`, `db/query/*.rs` |

**Total: ~23 `cfg` blocks, concentrated in 4 files. Zero cfg in business logic.**
