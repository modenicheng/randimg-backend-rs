# Root Task Derived Status — DB-Level Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Root tasks display a derived status aggregated from their entire descendant subtree (running/partial_success/failed/completed), with all computation and pagination at the DB layer.

**Architecture:** A single recursive CTE computes descendant status flags per root using SQL aggregation (`BOOL_OR` / `CASE WHEN`). The query returns root job columns + 3 boolean flags (`has_active`, `has_failed`, `has_completed`). A Rust helper maps the flags to derived status strings. The handler filters by derived status and paginates entirely in SQL.

**Tech Stack:** SeaORM raw SQL (`Statement::from_string`), recursive CTE (SQLite + PostgreSQL), Axum handlers, Vue 3 frontend.

---

## Context & Audit

### Problem
Root tasks currently show their own Apalis status ("Done") regardless of child states. A crawl that completed with some failed downloads still shows "completed". We need **derived status** that aggregates the descendant subtree.

### Why status overwrite is acceptable
- The platform has not launched — no backward compatibility constraint.
- `GET /tasks` (flat list) continues to return raw Apalis status for all jobs.
- `GET /tasks/roots` returns derived status — semantically, "status of this task tree".
- Frontend uses `status` field for display on both endpoints, so overwriting is correct for the roots view.
- A new `rawStatus` field preserves the root's own Apalis status for debugging.

### Previous implementation issues
1. **N+1 queries**: `batch_derived_statuses` loads ALL `(root_id, descendant_status)` pairs into Rust HashMap, then aggregates in Rust. For 100K descendants = 100K string allocations.
2. **No DB-level pagination**: Handler fetches up to 10000 roots, filters/paginates in Rust.
3. **`UNION ALL` in CTE**: Without dedup, a descendant reachable via multiple paths is counted multiple times (harmless for boolean flags but wasteful for large trees).

### Solution
Single recursive CTE with `GROUP BY root_id` + conditional aggregation. Returns `(root_job_columns, has_active, has_failed, has_completed)`. Filtering and pagination in SQL.

---

## Files to Modify

| File | Change |
|------|--------|
| `src/db/query/task_tree.rs` | Add `list_roots_derived()` — recursive CTE query returning root + flags |
| `src/handlers/task.rs` | Rewrite `list_roots` to use `list_roots_derived`, map flags to derived status |
| `src/views/TaskManager.vue` | Already updated (statusColor, statusLabel, statusItems, statusOrder) — verify only |

---

### Task 1: Add `list_roots_derived` to `task_tree.rs`

**Files:**
- Modify: `src/db/query/task_tree.rs`

**Context:** The existing `list_roots` function uses SeaORM's query builder for simple root filtering. The new `list_roots_derived` uses raw SQL for the recursive CTE. Both coexist — `list_roots` is still used by `get_subtasks` and other callers that don't need derived status.

- [ ] **Step 1: Add the `RootWithDerivedStatus` struct**

Add after the `ChildSummary` struct (around line 35):

```rust
/// Root task row with derived status flags computed from the descendant subtree.
///
/// The three boolean flags are computed in SQL via a recursive CTE.
/// The Rust side maps them to a derived status string.
#[derive(Debug, Clone)]
pub struct RootWithDerivedStatus {
    pub id: String,
    pub job_type: String,
    pub status: String,        // raw Apalis status (e.g. "Done")
    pub attempts: i32,
    pub max_attempts: i32,
    pub run_at: i64,           // unix timestamp (sqlite) — feature-gated below
    pub done_at: Option<i64>,
    pub last_result: Option<String>,
    pub priority: i32,
    pub job: Vec<u8>,          // serialized payload blob
    pub has_active: bool,
    pub has_failed: bool,
    pub has_completed: bool,
}
```

Note: The struct uses `i64` for timestamps (SQLite). For PostgreSQL, use feature-gated types. Since the project currently targets SQLite for dev, this is correct for the primary path. Add `#[cfg(feature = "postgres")]` variants if needed.

- [ ] **Step 2: Add the `derived_status_from_flags` helper**

Add after the struct:

```rust
/// Map the three descendant-status flags to a user-facing derived status string.
///
/// | has_active | has_failed | has_completed | derived          |
/// |------------|------------|---------------|------------------|
/// | true       | —          | —             | "running"        |
/// | false      | true       | true          | "partial_success"|
/// | false      | true       | false         | "failed"         |
/// | false      | false      | true          | "completed"      |
/// | false      | false      | false         | "pending"        |
pub fn derived_status_from_flags(has_active: bool, has_failed: bool, has_completed: bool) -> &'static str {
    if has_active {
        "running"
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
```

- [ ] **Step 3: Add the `list_roots_derived` function**

Add at the end of the file, replacing the existing `batch_derived_statuses` function:

```rust
pub async fn list_roots_derived(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    derived_status: Option<&str>,
    limit: u64,
    offset: u64,
) -> Result<Vec<RootWithDerivedStatus>, DbErr> {
    // Build optional WHERE clauses
    let mut extra_filters = String::new();

    if let Some(tt) = task_type {
        extra_filters.push_str(&format!(" AND r.job_type = '{}'", tt.replace('\'', "''")));
    }

    match derived_status {
        Some("running") => {
            extra_filters.push_str(" AND COALESCE(rf.has_active, 0) = 1");
        }
        Some("partial_success") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 1 \
                 AND COALESCE(rf.has_completed, 0) = 1",
            );
        }
        Some("failed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 1 \
                 AND COALESCE(rf.has_completed, 0) = 0",
            );
        }
        Some("completed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 0 \
                 AND COALESCE(rf.has_completed, 0) = 1",
            );
        }
        Some("pending") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 0 \
                 AND COALESCE(rf.has_completed, 0) = 0",
            );
        }
        Some(_) => {
            // Unknown status → return no results
            extra_filters.push_str(" AND 1 = 0");
        }
        None => {}
    }

    #[cfg(feature = "sqlite")]
    let sql = format!(
        r#"
        WITH RECURSIVE
            descendants AS (
                SELECT td.parent_job_id AS root_id,
                       td.child_job_id AS descendant_id
                FROM task_dependencies td
                WHERE td.parent_job_id NOT IN (
                    SELECT child_job_id FROM task_dependencies
                )
                UNION ALL
                SELECT d.root_id, td.child_job_id
                FROM descendants d
                JOIN task_dependencies td ON td.parent_job_id = d.descendant_id
            ),
            root_flags AS (
                SELECT
                    d.root_id,
                    MAX(CASE WHEN j2.status IN ('Pending','Queued','Running') THEN 1 ELSE 0 END) AS has_active,
                    MAX(CASE WHEN j2.status IN ('Failed','Killed')              THEN 1 ELSE 0 END) AS has_failed,
                    MAX(CASE WHEN j2.status = 'Done'                            THEN 1 ELSE 0 END) AS has_completed
                FROM descendants d
                JOIN Jobs j2 ON j2.id = d.descendant_id
                GROUP BY d.root_id
            )
        SELECT
            r.id, r.job_type, r.status, r.attempts, r.max_attempts,
            r.run_at, r.done_at, r.last_result, r.priority, r.job,
            COALESCE(rf.has_active, 0)    AS has_active,
            COALESCE(rf.has_failed, 0)    AS has_failed,
            COALESCE(rf.has_completed, 0) AS has_completed
        FROM Jobs r
        LEFT JOIN root_flags rf ON rf.root_id = r.id
        WHERE r.id NOT IN (SELECT child_job_id FROM task_dependencies)
        {extra_filters}
        ORDER BY r.run_at DESC
        LIMIT {limit} OFFSET {offset}
        "#
    );

    #[cfg(feature = "postgres")]
    let sql = format!(
        r#"
        WITH RECURSIVE
            descendants AS (
                SELECT td.parent_job_id AS root_id,
                       td.child_job_id AS descendant_id
                FROM task_dependencies td
                WHERE td.parent_job_id NOT IN (
                    SELECT child_job_id FROM task_dependencies
                )
                UNION ALL
                SELECT d.root_id, td.child_job_id
                FROM descendants d
                JOIN task_dependencies td ON td.parent_job_id = d.descendant_id
            ),
            root_flags AS (
                SELECT
                    d.root_id,
                    BOOL_OR(j2.status IN ('Pending','Queued','Running')) AS has_active,
                    BOOL_OR(j2.status IN ('Failed','Killed'))            AS has_failed,
                    BOOL_OR(j2.status = 'Done')                          AS has_completed
                FROM descendants d
                JOIN apalis.jobs j2 ON j2.id = d.descendant_id
                GROUP BY d.root_id
            )
        SELECT
            r.id, r.job_type, r.status, r.attempts, r.max_attempts,
            r.run_at, r.done_at, r.last_result, r.priority, r.job,
            COALESCE(rf.has_active, false)    AS has_active,
            COALESCE(rf.has_failed, false)    AS has_failed,
            COALESCE(rf.has_completed, false) AS has_completed
        FROM apalis.jobs r
        LEFT JOIN root_flags rf ON rf.root_id = r.id
        WHERE r.id NOT IN (SELECT child_job_id FROM task_dependencies)
        {extra_filters}
        ORDER BY r.run_at DESC
        LIMIT {limit} OFFSET {offset}
        "#
    );

    let stmt = Statement::from_string(db.get_database_backend(), sql);
    let rows = db.query_all(stmt).await?;

    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(RootWithDerivedStatus {
            id: row.try_get_by_index(0)?,
            job_type: row.try_get_by_index(1)?,
            status: row.try_get_by_index(2)?,
            attempts: row.try_get_by_index(3)?,
            max_attempts: row.try_get_by_index(4)?,
            run_at: row.try_get_by_index(5)?,
            done_at: row.try_get_by_index(6)?,
            last_result: row.try_get_by_index(7)?,
            priority: row.try_get_by_index(8)?,
            job: row.try_get_by_index(9)?,
            has_active: row.try_get_by_index::<i32>(10)? != 0,
            has_failed: row.try_get_by_index::<i32>(11)? != 0,
            has_completed: row.try_get_by_index::<i32>(12)? != 0,
        });
    }

    Ok(results)
}

/// Count root tasks matching a derived status filter.
///
/// Uses the same CTE as `list_roots_derived` but returns only the count.
pub async fn count_roots_derived(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    derived_status: Option<&str>,
) -> Result<u64, DbErr> {
    let mut extra_filters = String::new();

    if let Some(tt) = task_type {
        extra_filters.push_str(&format!(" AND r.job_type = '{}'", tt.replace('\'', "''")));
    }

    match derived_status {
        Some("running") => {
            extra_filters.push_str(" AND COALESCE(rf.has_active, 0) = 1");
        }
        Some("partial_success") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 1 \
                 AND COALESCE(rf.has_completed, 0) = 1",
            );
        }
        Some("failed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 1 \
                 AND COALESCE(rf.has_completed, 0) = 0",
            );
        }
        Some("completed") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 0 \
                 AND COALESCE(rf.has_completed, 0) = 1",
            );
        }
        Some("pending") => {
            extra_filters.push_str(
                " AND COALESCE(rf.has_active, 0) = 0 \
                 AND COALESCE(rf.has_failed, 0) = 0 \
                 AND COALESCE(rf.has_completed, 0) = 0",
            );
        }
        Some(_) => {
            extra_filters.push_str(" AND 1 = 0");
        }
        None => {}
    }

    #[cfg(feature = "sqlite")]
    let sql = format!(
        r#"
        WITH RECURSIVE
            descendants AS (
                SELECT td.parent_job_id AS root_id,
                       td.child_job_id AS descendant_id
                FROM task_dependencies td
                WHERE td.parent_job_id NOT IN (
                    SELECT child_job_id FROM task_dependencies
                )
                UNION ALL
                SELECT d.root_id, td.child_job_id
                FROM descendants d
                JOIN task_dependencies td ON td.parent_job_id = d.descendant_id
            ),
            root_flags AS (
                SELECT
                    d.root_id,
                    MAX(CASE WHEN j2.status IN ('Pending','Queued','Running') THEN 1 ELSE 0 END) AS has_active,
                    MAX(CASE WHEN j2.status IN ('Failed','Killed')              THEN 1 ELSE 0 END) AS has_failed,
                    MAX(CASE WHEN j2.status = 'Done'                            THEN 1 ELSE 0 END) AS has_completed
                FROM descendants d
                JOIN Jobs j2 ON j2.id = d.descendant_id
                GROUP BY d.root_id
            )
        SELECT COUNT(*) AS cnt
        FROM Jobs r
        LEFT JOIN root_flags rf ON rf.root_id = r.id
        WHERE r.id NOT IN (SELECT child_job_id FROM task_dependencies)
        {extra_filters}
        "#
    );

    #[cfg(feature = "postgres")]
    let sql = format!(
        r#"
        WITH RECURSIVE
            descendants AS (
                SELECT td.parent_job_id AS root_id,
                       td.child_job_id AS descendant_id
                FROM task_dependencies td
                WHERE td.parent_job_id NOT IN (
                    SELECT child_job_id FROM task_dependencies
                )
                UNION ALL
                SELECT d.root_id, td.child_job_id
                FROM descendants d
                JOIN task_dependencies td ON td.parent_job_id = d.descendant_id
            ),
            root_flags AS (
                SELECT
                    d.root_id,
                    BOOL_OR(j2.status IN ('Pending','Queued','Running')) AS has_active,
                    BOOL_OR(j2.status IN ('Failed','Killed'))            AS has_failed,
                    BOOL_OR(j2.status = 'Done')                          AS has_completed
                FROM descendants d
                JOIN apalis.jobs j2 ON j2.id = d.descendant_id
                GROUP BY d.root_id
            )
        SELECT COUNT(*) AS cnt
        FROM apalis.jobs r
        LEFT JOIN root_flags rf ON rf.root_id = r.id
        WHERE r.id NOT IN (SELECT child_job_id FROM task_dependencies)
        {extra_filters}
        "#
    );

    let stmt = Statement::from_string(db.get_database_backend(), sql);
    let row = db.query_one(stmt).await?;

    match row {
        Some(r) => Ok(r.try_get_by_index::<i64>(0)? as u64),
        None => Ok(0),
    }
}
```

- [ ] **Step 4: Remove the old `batch_derived_statuses` function**

Delete the `batch_derived_statuses` function (lines 336–445 in the current file) and the `use std::collections::HashMap;` import.

- [ ] **Step 5: Verify build compiles**

Run: `cargo build`
Expected: Clean compile with no errors.

---

### Task 2: Rewrite `list_roots` handler

**Files:**
- Modify: `src/handlers/task.rs`

**Context:** The handler currently fetches all roots, calls `batch_derived_statuses`, and filters/paginates in Rust. Replace with a single call to `list_roots_derived` + `count_roots_derived`.

- [ ] **Step 1: Add `row_to_json_derived` helper**

Add after the existing `tree_row_to_json` function (around line 437):

```rust
/// Convert a `RootWithDerivedStatus` to JSON for the `/tasks/roots` response.
///
/// The `status` field contains the **derived** status (from descendant aggregation).
/// The root's own Apalis status is preserved in `rawStatus`.
fn row_to_json_derived(r: &query::task_tree::RootWithDerivedStatus) -> serde_json::Value {
    let run_at = fmt_ts(r.run_at);
    let done_at = r.done_at.and_then(fmt_ts);
    let last_result = r.last_result.clone();
    let payload = serde_json::from_slice::<serde_json::Value>(&r.job).ok();

    let derived = query::task_tree::derived_status_from_flags(
        r.has_active,
        r.has_failed,
        r.has_completed,
    );

    // If root has no descendants, use its own status as the derived status
    let effective_derived = if !r.has_active && !r.has_failed && !r.has_completed {
        map_status(&r.status)
    } else {
        derived
    };

    serde_json::json!({
        "id": r.id,
        "job_type": r.job_type,
        "status": effective_derived,
        "raw_status": map_status(&r.status),
        "priority": r.priority,
        "attempts": r.attempts,
        "max_attempts": r.max_attempts,
        "run_at": run_at,
        "done_at": done_at,
        "last_result": last_result,
        "payload": payload,
    })
}
```

Note: This returns `snake_case` keys (`job_type`, `run_at`, etc.) matching the existing `row_to_json` convention. The frontend's `parseTask` handles both camelCase and snake_case.

- [ ] **Step 2: Rewrite the `list_roots` handler**

Replace the existing `list_roots` function:

```rust
pub async fn list_roots(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<RootsOrSubtasksQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let limit = q.limit.unwrap_or(50).min(200);
    let offset = q.offset.unwrap_or(0);
    let db = &state.db;

    let (rows, total) = tokio::try_join!(
        query::task_tree::list_roots_derived(
            db,
            q.task_type.as_deref(),
            q.status.as_deref(),
            limit,
            offset,
        ),
        query::task_tree::count_roots_derived(
            db,
            q.task_type.as_deref(),
            q.status.as_deref(),
        ),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let items: Vec<serde_json::Value> = rows.iter().map(row_to_json_derived).collect();

    Ok(Json(serde_json::json!({
        "tasks": items,
        "total": total,
    })))
}
```

- [ ] **Step 3: Verify build compiles**

Run: `cargo build`
Expected: Clean compile. The old `batch_derived_statuses` import is no longer used and should have been removed in Task 1 Step 4.

---

### Task 3: Verify frontend compatibility

**Files:**
- Read-only: `src/views/TaskManager.vue`

- [ ] **Step 1: Verify `parseTask` handles the new response shape**

The frontend `parseTask` reads `raw.status` which maps to `raw["status"]`. The new response has:
- `"status": "partial_success"` (derived) — used for display ✅
- `"raw_status": "completed"` (root's own status) — not read by frontend, but available

The `statusColor`, `statusLabel`, `statusItems`, `statusOrder` already include `partial_success` from the previous implementation. No changes needed.

- [ ] **Step 2: Verify `cleanFlagItems` does NOT include `partial_success` in clean operations**

The `POST /tasks/clean` endpoint deletes by raw Apalis status. `partial_success` is not a raw status — it's a derived state. The frontend's `cleanFlagItems` should NOT include `partial_success`. Verify it does not. (It should only have: completed, failed, cancelled, pending, running.)

Actually — looking at the current code, `cleanFlagItems` was updated to include `部分成功` / `partial_success` in a previous edit. This is **incorrect** — the clean endpoint operates on raw Apalis statuses, not derived statuses. This needs to be reverted.

- [ ] **Step 3: Remove `partial_success` from `cleanFlagItems`**

Remove the `{ title: '部分成功', value: 'partial_success' }` entry from `cleanFlagItems`.

---

### Task 4: Build and test

- [ ] **Step 1: Build**

Run: `cargo build`
Expected: Clean compile, no warnings.

- [ ] **Step 2: Run all tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/db/query/task_tree.rs src/handlers/task.rs src/views/TaskManager.vue
git commit -m "feat: root task derived status via SQL CTE with DB-level pagination

- Add list_roots_derived() using recursive CTE + conditional aggregation
- Compute has_active/has_failed/has_completed flags entirely in SQL
- Filter by derived status and paginate at DB level (no Rust-side loading)
- Handler returns derived status in 'status' field, raw in 'raw_status'
- Frontend already supports partial_success display
- Remove partial_success from clean operations (not a raw status)"
```
