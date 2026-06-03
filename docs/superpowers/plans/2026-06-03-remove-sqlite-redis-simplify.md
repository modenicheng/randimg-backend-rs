# Remove SQLite Support & Redis, Simplify Code

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove all SQLite support, remove dead Redis code, delete Apalis leftovers, simplify cfg-gated code to PostgreSQL-only, and sync config files.

**Architecture:** PostgreSQL-only for both API database (SeaORM) and queue database (Fang). Remove `db-sqlite` feature flag and all conditional compilation. Remove `redis_url` (dead code from pre-Fang era). Delete `apalis_job` entity/query files (leftover from Apalis→Fang migration). Rewrite `task_tree.rs` to use the `task` entity instead of `apalis_job`.

**Tech Stack:** Rust, SeaORM, Fang, PostgreSQL

---

## File Map

### Files to DELETE entirely:
- `crates/randimg-core/src/db/entities/apalis_job.rs` — Apalis entity (replaced by task entity)
- `crates/randimg-core/src/db/query/apalis_job.rs` — Apalis queries (replaced by task queries)
- `crates/randimg-core/src/db/query/task_tree.rs` — Apalis-based tree queries (rewrite to use task entity)

### Files to CREATE:
- `crates/randimg-core/src/db/query/task_tree.rs` — New tree queries using task entity

### Files to MODIFY:
- `crates/randimg-core/src/config.rs` — Remove `redis_url`, `database_url` legacy field
- `crates/randimg-core/src/db_backend.rs` — Remove `compile_error!` for mutual exclusion
- `crates/randimg-core/src/db/mod.rs` — Remove SQLite WAL pragma block
- `crates/randimg-core/src/db/entities/mod.rs` — Remove `apalis_job` module
- `crates/randimg-core/src/db/query/mod.rs` — Remove `apalis_job` module
- `crates/randimg-core/src/db/query/task.rs` — Remove SQLite cfg in `find_crawl_ids_by_type`
- `crates/randimg-core/src/handlers/task.rs` — Remove `apalis_job` import, use task entity for tree endpoints
- `crates/randimg-core/Cargo.toml` — Remove `db-sqlite` feature, default to `db-postgres`
- `crates/randimg-server/Cargo.toml` — Remove `db-sqlite` feature, default to `db-postgres`
- `crates/randimg-worker/Cargo.toml` — Remove `db-sqlite` feature, default to `db-postgres`
- `migration/Cargo.toml` — Remove `sqlite` feature, default to `postgres`
- `migration/src/lib.rs` — Remove `compile_error!` for mutual exclusion
- `migration/src/m20260531_000002_schema_refactor.rs` — Remove `#[cfg(feature = "sqlite")]` blocks
- `migration/src/m20260531_000005_add_deleted_at.rs` — Remove `#[cfg(feature = "sqlite")]` block
- `crates/randimg-server/src/main.rs` — Remove cfg env_name, use `config.api_database_url`
- `crates/randimg-core/tests/db_test.rs` — Remove `redis_url`, update DB URLs to postgres
- `crates/randimg-core/tests/handler_test.rs` — Remove `redis_url`, update DB URLs to postgres
- `.env.example` — Remove `DATABASE_URL`, `REDIS_URL`, dead env vars; sync with config.rs
- `README.md` — Update build instructions to remove sqlite feature references
- `CLAUDE.md` — Update feature flag docs

---

## Task 1: Remove `redis_url` from config and tests

**Files:**
- Modify: `crates/randimg-core/src/config.rs:52,122-123`
- Modify: `crates/randimg-core/tests/db_test.rs:467`
- Modify: `crates/randimg-core/tests/handler_test.rs:51`

- [ ] **Step 1: Remove `redis_url` field from AppConfig struct**

In `crates/randimg-core/src/config.rs`, delete line 52:
```rust
    pub redis_url: String,
```

- [ ] **Step 2: Remove `redis_url` initialization from `from_env()`**

Delete lines 122-123:
```rust
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".into()),
```

- [ ] **Step 3: Remove `redis_url` from test fixtures**

In `crates/randimg-core/tests/db_test.rs`, delete line 467:
```rust
        redis_url: "redis://127.0.0.1:6379".into(),
```

In `crates/randimg-core/tests/handler_test.rs`, delete line 51:
```rust
        redis_url: "redis://127.0.0.1:6379".into(),
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`
Expected: Clean (no errors)

---

## Task 2: Remove `database_url` legacy field

**Files:**
- Modify: `crates/randimg-core/src/config.rs:46-47,112-113,116-118`
- Modify: `crates/randimg-server/src/main.rs:66,193,235,252`
- Modify: `crates/randimg-core/tests/db_test.rs:440`
- Modify: `crates/randimg-core/tests/handler_test.rs:24`

- [ ] **Step 1: Remove `database_url` field from AppConfig struct**

In `crates/randimg-core/src/config.rs`, delete lines 46-47:
```rust
    /// 旧版单一数据库 URL（保留用于向后兼容）
    pub database_url: String,
```

- [ ] **Step 2: Simplify `from_env()` — remove legacy fallback**

Replace the legacy_database_url block (lines 111-118) with direct reads:
```rust
        Self {
            api_database_url: env::var("API_DATABASE_URL")
                .unwrap_or_else(|_| "postgres://localhost/randimg".into()),
            queue_database_url: env::var("QUEUE_DATABASE_URL")
                .unwrap_or_else(|_| "postgres://localhost/randimg_queue".into()),
```

Remove the `legacy_database_url` variable and the `database_url:` line.

- [ ] **Step 3: Update `server/main.rs` references**

Replace all `config.database_url` with `config.api_database_url`:
- Line 66: `db::init_database(&config.api_database_url)`
- Line 193: `config.api_database_url` in startup banner
- Line 235: `config.api_database_url` in TCP startup log
- Line 252: `config.api_database_url` in Unix socket startup log

- [ ] **Step 4: Update test fixtures**

In `db_test.rs` and `handler_test.rs`, remove `database_url` field from `make_test_config()` and update `api_database_url` to `"postgres://localhost/test_db"`.

- [ ] **Step 5: Verify compilation**

Run: `cargo check`
Expected: Clean

---

## Task 3: Delete Apalis leftover files and modules

**Files:**
- Delete: `crates/randimg-core/src/db/entities/apalis_job.rs`
- Delete: `crates/randimg-core/src/db/query/apalis_job.rs`
- Modify: `crates/randimg-core/src/db/entities/mod.rs:2`
- Modify: `crates/randimg-core/src/db/query/mod.rs:2`

- [ ] **Step 1: Delete `entities/apalis_job.rs`**

```bash
rm crates/randimg-core/src/db/entities/apalis_job.rs
```

- [ ] **Step 2: Delete `query/apalis_job.rs`**

```bash
rm crates/randimg-core/src/db/query/apalis_job.rs
```

- [ ] **Step 3: Remove module declarations**

In `crates/randimg-core/src/db/entities/mod.rs`, delete:
```rust
pub mod apalis_job;
```

In `crates/randimg-core/src/db/query/mod.rs`, delete:
```rust
pub mod apalis_job;
```

- [ ] **Step 4: Verify compilation (will show errors in task_tree.rs and handlers/task.rs)**

Run: `cargo check 2>&1 | head -30`
Expected: Errors in `task_tree.rs` and `handlers/task.rs` referencing `apalis_job` — these are fixed in the next task.

---

## Task 4: Rewrite `task_tree.rs` to use `task` entity

**Files:**
- Delete + Create: `crates/randimg-core/src/db/query/task_tree.rs`

- [ ] **Step 1: Delete old `task_tree.rs`**

```bash
rm crates/randimg-core/src/db/query/task_tree.rs
```

- [ ] **Step 2: Write new `task_tree.rs` using `task` entity**

The new module provides the same tree query API but uses the `task` entity (which has `root_id` for flat tree queries, `parent_id` for direct parent-child).

```rust
//! Task tree queries — root tasks with derived status, subtask listing, interrupt.
//!
//! Uses the `task` entity with `root_id` for flat tree queries and `parent_id`
//! for direct parent-child relationships. No recursive CTE needed — the
//! `root_id` column enables single-query tree traversal.

use crate::db::entities::task::{self, Entity as Task, STATUS_DONE, STATUS_FAILED, STATUS_KILLED, STATUS_PENDING, STATUS_QUEUED, STATUS_RUNNING};
use sea_orm::*;
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A node in the task hierarchy tree, holding both the serialized task data
/// and any child nodes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChildTaskNode {
    /// Task information as JSON (id, task_type, status, params, …).
    pub task: JsonValue,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ChildTaskNode>,
}

/// Root task row with derived status flags computed from the descendant subtree.
#[derive(Debug, Clone)]
pub struct RootWithDerivedStatus {
    pub id: String,
    pub task_type: String,
    pub status: String,
    pub root_id: Option<String>,
    pub crawler_id: Option<i32>,
    pub image_id: Option<i32>,
    pub params: Option<String>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub has_active: bool,
    pub has_failed: bool,
    pub has_completed: bool,
    pub has_killed_terminal: bool,
}

/// Map the descendant-status flags to a user-facing derived status string.
pub fn derived_status_from_flags(
    has_active: bool,
    has_failed: bool,
    has_completed: bool,
    has_killed_terminal: bool,
) -> &'static str {
    if has_active {
        "running"
    } else if has_failed && !has_completed && has_killed_terminal == has_failed {
        "killed"
    } else if has_failed && has_completed {
        "partial_success"
    } else if has_failed {
        "failed"
    } else if has_completed {
        "completed"
    } else {
        "pending"
    }
}

/// Convert a `task::Model` into a JSON value suitable for API responses.
pub fn model_to_json(m: &task::Model) -> JsonValue {
    let payload = m.params.as_ref()
        .and_then(|s| serde_json::from_str::<JsonValue>(s).ok());

    serde_json::json!({
        "id":             m.id,
        "taskType":       m.task_type,
        "status":         m.status,
        "parentId":       m.parent_id,
        "rootId":         m.root_id,
        "crawlerId":      m.crawler_id,
        "imageId":        m.image_id,
        "params":         payload,
        "errorMessage":   m.error_message,
        "retryCount":     m.retry_count,
        "createdAt":      m.created_at.to_rfc3339(),
        "updatedAt":      m.updated_at.to_rfc3339(),
        "completedAt":    m.completed_at.map(|dt| dt.to_rfc3339()),
    })
}

// ---------------------------------------------------------------------------
// Root listing with derived status
// ---------------------------------------------------------------------------

/// List root tasks with derived status flags computed from the descendant subtree.
///
/// Root tasks are those with `parent_id IS NULL`. Derived status is computed
/// by aggregating child statuses via the `root_id` column.
pub async fn list_roots_derived(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    crawl_type: Option<i32>,
    derived_status: Option<&str>,
    limit: u64,
    offset: u64,
) -> Result<Vec<RootWithDerivedStatus>, DbErr> {
    // Step 1: Fetch root tasks
    let mut q = Task::find()
        .filter(task::Column::ParentId.is_null())
        .order_by_desc(task::Column::CreatedAt);

    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }

    let roots = q.limit(limit).offset(offset).all(db).await?;

    // Step 2: For each root, compute derived status from children
    let mut results = Vec::with_capacity(roots.len());
    for root in roots {
        let children: Vec<task::Model> = Task::find()
            .filter(task::Column::RootId.eq(&root.id))
            .filter(task::Column::Id.ne(&root.id))
            .all(db)
            .await?;

        let mut has_active = false;
        let mut has_failed = false;
        let mut has_completed = false;
        let mut has_killed_terminal = false;

        for child in &children {
            match child.status.as_str() {
                STATUS_PENDING | STATUS_QUEUED | STATUS_RUNNING => has_active = true,
                STATUS_DONE => has_completed = true,
                STATUS_FAILED => has_failed = true,
                STATUS_KILLED => {
                    has_failed = true;
                    has_killed_terminal = true;
                }
                _ => {}
            }
        }

        // Filter by crawl_type if specified (check params JSON)
        if let Some(ct) = crawl_type {
            let matches = root.params.as_ref()
                .and_then(|p| serde_json::from_str::<JsonValue>(p).ok())
                .and_then(|v| v.get("crawl_type").and_then(|c| c.as_i64()))
                .map(|v| v == ct as i64)
                .unwrap_or(false);
            if !matches {
                continue;
            }
        }

        // Filter by derived_status if specified
        if let Some(ds) = derived_status {
            let computed = derived_status_from_flags(has_active, has_failed, has_completed, has_killed_terminal);
            if computed != ds {
                continue;
            }
        }

        results.push(RootWithDerivedStatus {
            id: root.id,
            task_type: root.task_type,
            status: root.status,
            root_id: root.root_id,
            crawler_id: root.crawler_id,
            image_id: root.image_id,
            params: root.params,
            error_message: root.error_message,
            retry_count: root.retry_count,
            created_at: root.created_at,
            updated_at: root.updated_at,
            completed_at: root.completed_at,
            has_active,
            has_failed,
            has_completed,
            has_killed_terminal,
        });
    }

    Ok(results)
}

/// Count root tasks with derived status filters applied.
pub async fn count_roots_derived(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    crawl_type: Option<i32>,
    derived_status: Option<&str>,
) -> Result<u64, DbErr> {
    // Reuse list_roots_derived with a large limit, then count
    // (For simplicity — a dedicated SQL COUNT would be more efficient for large datasets)
    let roots = list_roots_derived(db, task_type, crawl_type, derived_status, 10_000, 0).await?;
    Ok(roots.len() as u64)
}

// ---------------------------------------------------------------------------
// Children → full task details (hierarchical)
// ---------------------------------------------------------------------------

/// Return all child tasks for `root_id`, optionally filtered by type/status.
///
/// Uses `root_id` for flat tree traversal — no recursive CTE needed.
/// Returns results as a nested tree structure.
pub async fn list_children(
    db: &DatabaseConnection,
    root_id: &str,
    task_type: Option<&str>,
    status: Option<&[&str]>,
    max_depth: u32,
) -> Result<Vec<ChildTaskNode>, DbErr> {
    if max_depth == 0 {
        return Ok(vec![]);
    }

    // Get all descendants via root_id
    let mut q = Task::find()
        .filter(task::Column::RootId.eq(root_id))
        .filter(task::Column::Id.ne(root_id))
        .order_by_asc(task::Column::CreatedAt);

    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(task::Column::Status.is_in(sts.iter().copied()));
    }

    let all_descendants = q.all(db).await?;

    // Build adjacency list: parent_id → children
    let mut children_map: std::collections::HashMap<String, Vec<&task::Model>> =
        std::collections::HashMap::new();
    for d in &all_descendants {
        if let Some(ref pid) = d.parent_id {
            children_map.entry(pid.clone()).or_default().push(d);
        }
    }

    // Recursive tree builder
    fn build_tree(
        parent_id: &str,
        children_map: &std::collections::HashMap<String, Vec<&task::Model>>,
        depth: u32,
    ) -> Vec<ChildTaskNode> {
        if depth == 0 {
            return vec![];
        }
        let Some(children) = children_map.get(parent_id) else {
            return vec![];
        };
        children
            .iter()
            .map(|child| ChildTaskNode {
                task: model_to_json(child),
                children: build_tree(&child.id, children_map, depth - 1),
            })
            .collect()
    }

    Ok(build_tree(root_id, &children_map, max_depth))
}

// ---------------------------------------------------------------------------
// Subtasks (flat, non-recursive — direct children only)
// ---------------------------------------------------------------------------

/// Return direct child tasks for a given parent, with optional filters.
pub async fn list_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&[&str]>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<task::Model>, DbErr> {
    let mut q = Task::find()
        .filter(task::Column::ParentId.eq(parent_id))
        .order_by_desc(task::Column::CreatedAt);

    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(task::Column::Status.is_in(sts.iter().copied()));
    }
    if let Some(l) = limit {
        q = q.limit(l);
    }
    if let Some(o) = offset {
        q = q.offset(o);
    }

    q.all(db).await
}

/// Count direct child tasks for a given parent.
pub async fn count_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
    status: Option<&[&str]>,
) -> Result<u64, DbErr> {
    let mut q = Task::find()
        .filter(task::Column::ParentId.eq(parent_id));

    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }
    if let Some(sts) = status {
        q = q.filter(task::Column::Status.is_in(sts.iter().copied()));
    }

    q.count(db).await
}

// ---------------------------------------------------------------------------
// Interrupt (delete) pending subtasks
// ---------------------------------------------------------------------------

/// Delete all pending children of `parent_id`, optionally filtered by task_type.
///
/// Returns the list of deleted task IDs.
pub async fn interrupt_subtasks(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: Option<&str>,
) -> Result<(Vec<String>, u64), DbErr> {
    let mut q = Task::find()
        .filter(task::Column::ParentId.eq(parent_id))
        .filter(task::Column::Status.is_in([STATUS_PENDING, STATUS_QUEUED]));

    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }

    let to_delete = q.all(db).await?;
    let deleted_ids: Vec<String> = to_delete.iter().map(|t| t.id.clone()).collect();

    if deleted_ids.is_empty() {
        return Ok((vec![], 0));
    }

    // Delete task_dependency rows and task rows atomically
    use crate::db::entities::task_dependency::{Column as DepCol, Entity as TaskDependency};
    let ids_clone = deleted_ids.clone();
    let count = db.transaction::<_, u64, DbErr>(|txn| {
        Box::pin(async move {
            TaskDependency::delete_many()
                .filter(DepCol::ChildJobId.is_in(&ids_clone))
                .exec(txn)
                .await?;

            let result = Task::delete_many()
                .filter(task::Column::Id.is_in(&ids_clone))
                .exec(txn)
                .await?;

            Ok(result.rows_affected)
        })
    })
    .await
    .map_err(|e| match e {
        TransactionError::Connection(e) => e,
        TransactionError::Transaction(e) => e,
    })?;

    Ok((deleted_ids, count))
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: Clean

---

## Task 5: Update `handlers/task.rs` to use task entity

**Files:**
- Modify: `crates/randimg-core/src/handlers/task.rs:13,447-465,501-516,558,688-696`

- [ ] **Step 1: Remove `apalis_job` import**

Delete line 13:
```rust
use crate::db::entities::apalis_job;
```

- [ ] **Step 2: Remove `tree_row_to_json` function (lines 447-465)**

This function converts `apalis_job::Model` to JSON. Delete the entire function.

- [ ] **Step 3: Update `list_roots` handler (lines 489-579)**

The handler uses `task_tree::list_roots_derived` which now returns `RootWithDerivedStatus` with task entity fields. Update the JSON construction to match the new struct:

```rust
    let items: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let payload = r.params.as_ref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
            let derived = query::task_tree::derived_status_from_flags(
                r.has_active,
                r.has_failed,
                r.has_completed,
                r.has_killed_terminal,
            );

            let root_mapped = map_status(&r.status);
            let has_descendants = r.has_active || r.has_failed || r.has_completed;
            let subtree_dead_terminal = !r.has_active
                && r.has_failed
                && !r.has_completed
                && r.has_killed_terminal == r.has_failed;
            let effective = if subtree_dead_terminal {
                "killed"
            } else if root_mapped != "completed" {
                root_mapped
            } else if has_descendants {
                derived
            } else {
                "completed"
            };

            serde_json::json!({
                "id": r.id,
                "task_type": r.task_type,
                "status": effective,
                "raw_status": map_status(&r.status),
                "retry_count": r.retry_count,
                "created_at": r.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                "completed_at": r.completed_at.map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
                "error_message": r.error_message,
                "payload": payload,
            })
        })
        .collect();
```

- [ ] **Step 4: Update `get_subtasks` handler (lines 753-803)**

Replace `tree_row_to_json(&j)` with `row_to_json(&j)` (the existing function that works with `task::Model`):

```rust
    let page: Vec<serde_json::Value> = children
        .into_iter()
        .map(|j| row_to_json(&j))
        .collect();
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check`
Expected: Clean

---

## Task 6: Remove SQLite feature flags from Cargo.toml files

**Files:**
- Modify: `crates/randimg-core/Cargo.toml:11,14`
- Modify: `crates/randimg-server/Cargo.toml:32-33`
- Modify: `crates/randimg-worker/Cargo.toml:23-24`
- Modify: `migration/Cargo.toml:7-8`

- [ ] **Step 1: Update `crates/randimg-core/Cargo.toml`**

Change features section to:
```toml
[features]
default = ["db-postgres", "queue-postgres", "http"]

# Database backend for app data (SeaORM) — PostgreSQL only
db-postgres = ["migration/postgres", "sea-orm/sqlx-postgres"]

# Queue backend (PostgreSQL only — fang async API)
queue-postgres = ["fang/asynk-postgres"]

# HTTP layer (Axum handlers, auth middleware, error types) — only needed by server binary
http = ["dep:axum", "dep:tower-http", "dep:axum-extra"]
```

- [ ] **Step 2: Update `crates/randimg-server/Cargo.toml`**

```toml
[features]
default = ["db-postgres", "queue-postgres", "http"]
db-postgres = ["randimg-core/db-postgres"]
queue-postgres = ["randimg-core/queue-postgres"]
http = ["randimg-core/http"]
```

- [ ] **Step 3: Update `crates/randimg-worker/Cargo.toml`**

```toml
[features]
default = ["db-postgres", "queue-postgres"]
db-postgres = ["randimg-core/db-postgres"]
queue-postgres = ["randimg-core/queue-postgres"]
```

- [ ] **Step 4: Update `migration/Cargo.toml`**

```toml
[features]
default = ["postgres"]
postgres = ["sea-orm-migration/sqlx-postgres"]
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check`
Expected: Clean

---

## Task 7: Remove SQLite-specific code from `db_backend.rs` and `db/mod.rs`

**Files:**
- Modify: `crates/randimg-core/src/db_backend.rs:6-7`
- Modify: `crates/randimg-core/src/db/mod.rs:12-23`

- [ ] **Step 1: Remove `compile_error!` from `db_backend.rs`**

Delete lines 6-7:
```rust
#[cfg(all(feature = "db-sqlite", feature = "db-postgres"))]
compile_error!("Features 'db-sqlite' and 'db-postgres' are mutually exclusive. Use --no-default-features when enabling postgres.");
```

- [ ] **Step 2: Remove SQLite WAL pragma block from `db/mod.rs`**

Delete lines 12-23:
```rust
    // Enable WAL journal mode and set busy_timeout for better concurrent access.
    // WAL allows concurrent readers while a writer holds a lock, and busy_timeout
    // makes writers wait instead of immediately returning SQLITE_BUSY.
    #[cfg(feature = "db-sqlite")]
    {
        use sea_orm::{ConnectionTrait, Statement};
        for pragma in ["PRAGMA journal_mode=WAL", "PRAGMA busy_timeout=5000"] {
            db.execute(Statement::from_string(sea_orm::DatabaseBackend::Sqlite, pragma.to_string()))
                .await
                .expect("Failed to set SQLite pragma");
        }
    }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: Clean

---

## Task 8: Remove SQLite cfg from `db/query/task.rs`

**Files:**
- Modify: `crates/randimg-core/src/db/query/task.rs:237-267`

- [ ] **Step 1: Remove SQLite-specific `find_crawl_ids_by_type`**

Delete the `#[cfg(feature = "db-sqlite")]` block (lines 237-251) and the `#[cfg(feature = "db-postgres")]` attribute (line 253), keeping only the PostgreSQL implementation:

```rust
pub async fn find_crawl_ids_by_type(
    db: &DatabaseConnection,
    crawl_type: i32,
) -> Result<Vec<i32>, DbErr> {
    let sql = "SELECT crawler_id FROM tasks WHERE task_type = 'crawl' AND (params::json->>'crawl_type')::int = $1 AND crawler_id IS NOT NULL";
    let stmt = Statement::from_sql_and_values(
        db.get_database_backend(),
        sql,
        [crawl_type.into()],
    );
    let rows = db.query_all(stmt).await?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in &rows {
        ids.push(row.try_get_by_index::<i32>(0)?);
    }
    Ok(ids)
}
```

Also update the comment at line 2-4 to remove "支持 SQLite 和 PostgreSQL":
```rust
/// 任务查询函数
///
/// 提供 tasks 表的 CRUD 操作，替代 apalis_job 查询模块。
/// 使用 SeaORM 查询构建器。
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: Clean

---

## Task 9: Remove SQLite cfg from migration files

**Files:**
- Modify: `migration/src/lib.rs:1-2`
- Modify: `migration/src/m20260531_000002_schema_refactor.rs` (10 cfg blocks)
- Modify: `migration/src/m20260531_000005_add_deleted_at.rs:10-15`

- [ ] **Step 1: Remove `compile_error!` from `migration/src/lib.rs`**

Delete lines 1-2:
```rust
#[cfg(all(feature = "sqlite", feature = "postgres"))]
compile_error!("Features 'sqlite' and 'postgres' are mutually exclusive. Use --no-default-features when enabling postgres.");
```

- [ ] **Step 2: Simplify `m20260531_000002_schema_refactor.rs`**

Remove all `#[cfg(feature = "sqlite")]` / `#[cfg(feature = "postgres")]` pairs, keeping only the PostgreSQL SQL. The file has 6 pairs in `up()` and 4 pairs in `down()`.

In `up()`, for each pair keep only the `#[cfg(feature = "postgres")]` version and remove the cfg attribute:
- Lines 12-19: Keep `ALTER TABLE images ADD COLUMN is_public BOOLEAN NOT NULL DEFAULT false`
- Lines 25-32: Keep `ALTER TABLE images ADD COLUMN source_created_at TIMESTAMPTZ`
- Lines 50-57: Keep `ALTER TABLE images ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`
- Lines 62-73: Keep `UPDATE images SET is_public = CASE WHEN uploaded = true AND processed = true AND (accessable IS NULL OR accessable = true) THEN true ELSE false END`

In `down()`, for each pair keep only the PostgreSQL version:
- Lines 100-107: Keep `ALTER TABLE images ADD COLUMN uploaded BOOLEAN NOT NULL DEFAULT false`
- Lines 109-116: Keep `ALTER TABLE images ADD COLUMN downloaded BOOLEAN NOT NULL DEFAULT false`
- Lines 118-125: Keep `ALTER TABLE images ADD COLUMN processed BOOLEAN NOT NULL DEFAULT false`
- Lines 127-134: Keep `ALTER TABLE images ADD COLUMN processing BOOLEAN NOT NULL DEFAULT false`
- Lines 137-148: Keep `UPDATE images SET uploaded = CASE WHEN is_public = true THEN true ELSE false END, processed = CASE WHEN is_public = true THEN true ELSE false END`

- [ ] **Step 3: Simplify `m20260531_000005_add_deleted_at.rs`**

In `up()`, remove the cfg pair (lines 10-15), keep only:
```rust
        db.execute_unprepared("ALTER TABLE images ADD COLUMN deleted_at TIMESTAMPTZ")
            .await?;
```

In `down()`, the comment about SQLite can be removed (line 20-21). Keep the code as-is (it already works for PostgreSQL).

- [ ] **Step 4: Verify compilation**

Run: `cargo check`
Expected: Clean

---

## Task 10: Remove cfg env_name from `server/main.rs`

**Files:**
- Modify: `crates/randimg-server/src/main.rs:170-174`

- [ ] **Step 1: Remove cfg-based env_name**

Delete lines 170-173:
```rust
    #[cfg(feature = "db-sqlite")]
    let env_name = "development";
    #[cfg(feature = "db-postgres")]
    let env_name = "production";
```

Replace with:
```rust
    let env_name = "production";
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: Clean

---

## Task 11: Update `.env.example`

**Files:**
- Modify: `.env.example`

- [ ] **Step 1: Rewrite `.env.example`**

Remove `DATABASE_URL` (legacy), `REDIS_URL` (dead), `DOGECLOUD_SERVICE_TYPE` (unused in code), `COLOR_WORKER_AUTO_SPAWN` (unused in code). Update comments to reflect PostgreSQL-only:

```env
# ── Database ──────────────────────────────────────────────────────
# API 数据库 (SeaORM) — 存储业务数据和自定义任务表
API_DATABASE_URL=postgres://user:password@localhost/randimg

# 队列数据库 (Fang) — 存储 fang_tasks 表
QUEUE_DATABASE_URL=postgres://user:password@localhost/randimg_queue

# JWT secret key (REQUIRED - must be changed)
SECRET_KEY=change-me-to-a-random-secret

# JWT token expiration in minutes
JWT_EXPIRE_MINUTES=60

# CDN base URL for image serving
CDN_BASE_URL=https://cdn.modenc.top/

# Local image storage directory
IMAGE_DIR=./images

# Server listen address
# Formats: host:port (TCP), http://host:port (TCP), unix:///path/to.sock (Unix socket)
SERVER_ADDR=0.0.0.0:8000

# Pixiv API refresh token (for crawler)
PIXIV_REFRESH_TOKEN=

# Pixiv API proxy (optional)
PIXIV_PROXY=http://127.0.0.1:1080

# Pixiv Accept-Language header (enables translated_name in API responses)
# Values: zh-CN, en-us, ja, ko, etc.
PIXIV_ACCEPT_LANG=zh-CN

# Logging level (RUST_LOG format: target1=level,target2=level)
# Levels: trace, debug, info, warn, error
# Examples:
#   randimg_core=info                    (default - app info only)
#   randimg_core=debug,tower_http=info   (app debug + HTTP info)
#   randimg_core=trace                   (very verbose)
RUST_LOG=randimg_core=info,tower_http=info

# Log output directory (default: ./logs). Logs rotate daily.
# LOG_DIR=./logs

# JSON log format (set to true for production / log aggregation tools)
# LOG_JSON=false

# Task retry settings (exponential backoff)
# Maximum number of retries per failed job (default: 3)
# RETRY_MAX_RETRIES=3

# Minimum backoff delay in milliseconds (default: 1000)
# RETRY_BACKOFF_MIN_MS=1000

# Maximum backoff delay cap in seconds (default: 60)
# RETRY_BACKOFF_MAX_SECS=60

# Jitter factor 0.0-100.0, randomizes each delay to avoid thundering herd (default: 0.5)
# RETRY_BACKOFF_JITTER=0.5

# ── Fang 任务调度配置 ────────────────────────────────────────
# 最大重试次数（default: 3）
# TASK_MAX_RETRIES=3

# 退避基数 — 指数退避：base^n 秒（default: 2）
# TASK_BACKOFF_BASE=2

# 轮询间隔（毫秒）— Fang worker 检查新任务的频率（default: 500）
# TASK_POLL_INTERVAL_MS=500

# 默认超时时间（秒）— 超过此时间未完成的任务将被标记为失败（default: 300）
# TASK_DEFAULT_TIMEOUT_SECS=300

# ── 各任务类型并发数 ────────────────────────────────────────
# TASK_CONCURRENCY_CRAWL=2
# TASK_CONCURRENCY_DOWNLOAD=4
# TASK_CONCURRENCY_COLOR_EXTRACT=2
# TASK_CONCURRENCY_UPLOAD=2
# TASK_CONCURRENCY_ACCESSIBILITY_CHECK=2
# TASK_CONCURRENCY_DISCOVER=1
# TASK_CONCURRENCY_REFRESH_PIXIV_TOKEN=1

# DogeCloud OSS (permanent AccessKey / SecretKey for fetching temporary S3 credentials)
# Get these from: DogeCloud Console → User Center → Key Management
DOGECLOUD_ACCESS_KEY=
DOGECLOUD_SECRET_KEY=
# S3 Bucket name (fallback, API response takes priority when available)
DOGECLOUD_S3_BUCKET=
# S3 Endpoint (fallback, API response takes priority when available)
DOGECLOUD_S3_ENDPOINT=

# ── Color Worker Process Isolation ──────────────────────────
# Set to "true" to run color extraction in a separate binary.
# The main server will NOT spawn a color-extract worker.
# Run `cargo run -p randimg-worker` as a separate process.
# COLOR_WORKER_STANDALONE=false

# Number of rayon threads for color extraction (default: CPU count).
# Lower this to limit CPU usage, raise it to utilize more cores.
# COLOR_WORKER_RAYON_THREADS=4
```

- [ ] **Step 2: Verify no dead env vars remain**

Check that every env var in `.env.example` has a corresponding `env::var()` in `config.rs`.

---

## Task 12: Update documentation

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update `README.md` build/run instructions**

Remove all `--no-default-features --features db-sqlite` references. Update feature flag documentation to reflect PostgreSQL-only.

- [ ] **Step 2: Update `CLAUDE.md` feature flag docs**

Remove `db-sqlite` / `db-postgres` mutual exclusion warnings. Update to reflect single-database architecture.

---

## Task 13: Final verification

- [ ] **Step 1: Full compilation check**

Run: `cargo check`
Expected: Clean

- [ ] **Step 2: Run tests**

Run: `cargo test -p randimg-core --features db-postgres,queue-postgres -- --skip color_test`
Expected: All passing (or note pre-existing failures)

- [ ] **Step 3: Verify no remaining SQLite references**

Run: `grep -r "sqlite\|db-sqlite\|cfg.*sqlite" --include="*.rs" --include="*.toml" crates/ migration/`
Expected: No matches

- [ ] **Step 4: Verify no remaining Redis references**

Run: `grep -r "redis\|REDIS" --include="*.rs" --include="*.toml" --include=".env*" crates/ .env*`
Expected: No matches

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor: remove SQLite support and Redis, PostgreSQL-only

- Remove db-sqlite feature flag from all crates
- Remove redis_url from AppConfig (dead code)
- Remove legacy database_url field (use api_database_url directly)
- Delete apalis_job entity and query modules (leftover from migration)
- Rewrite task_tree.rs to use task entity instead of apalis_job
- Remove all #[cfg(feature = ...)] conditional compilation for DB backends
- Sync .env.example with actual config fields
- Remove dead env vars (DOGECLOUD_SERVICE_TYPE, COLOR_WORKER_AUTO_SPAWN)
- Update documentation to reflect PostgreSQL-only architecture"
```
