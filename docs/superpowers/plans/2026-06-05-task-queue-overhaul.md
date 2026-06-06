# Task Queue System Overhaul Plan
Created: 2026-06-05

## Overview
Comprehensive overhaul of the task queue system addressing 20 existing issues, 14 improvements, and adding reliability features. Excludes Web UI (already partially implemented via API).

## Architecture Context
- **Dual-table design**: Custom `tasks` table (SeaORM) + `fang_tasks` table (Fang)
- **7 task types**: crawl, download, color_extract, upload, accessibility_check, discover, refresh_pixiv_token
- **Status lifecycle**: pending → queued → running → done/failed/killed
- **Worker concurrency**: 14 total workers (2+4+2+2+2+1+1 per type)

---

## Parallel Task Graph

```
Wave 1 (Parallel - No dependencies):
├── T1: Add indexes (migration)
├── T2: Fix unwrap() usage
├── T3: Create CrawlType enum
└── T4: Remove task_dependencies table

Wave 2 (Parallel - After Wave 1):
├── T5: Macro for boilerplate
├── T6: Batch query optimization
└── T7: Transactional task push

Wave 3 (Sequential - After Wave 2):
├── T8: Graceful shutdown
└── T9: Task timeout mechanism

Wave 4 (Parallel - After Wave 3):
├── T10: Dead Letter Queue
├── T11: Watchdog
└── T12: Health endpoint

Wave 5 (Parallel - After Wave 4):
├── T13: Automatic cleanup
├── T14: Idempotency checks
├── T15: Auth retry improvement
└── T16: Discover deduplication

Wave 6 (Parallel - After Wave 5):
├── T17: Task Priority
├── T18: Task Progress Tracking
├── T19: Task Deduplication
└── T20: Task Metrics

Wave 7 (Sequential - After Wave 6):
├── T21: JSONB migration
└── T22: Enum constraints
```

---

## Wave 1: Critical Fixes (Parallel)

### T1: Add Indexes
**Files**: `migration/src/`
**Category**: `quick`
**Skills**: []
**Verification**: `cargo run --bin migrate` succeeds, `\d tasks` shows new indexes

**Tasks**:
1. Create migration `m20260605_001_add_task_indexes.rs`
2. Add index on `fang_task_id` (for reverse lookups)
3. Add index on `status` (for filtering)
4. Add composite index on `(root_id, parent_id)` (for tree queries)

---

### T2: Fix unwrap() Usage
**Files**: `crates/randimg-core/src/db/query/task.rs`
**Category**: `quick`
**Skills**: []
**Verification**: `cargo test -p randimg-core` passes

**Tasks**:
1. Find `increment_retry` function
2. Replace `active.retry_count.unwrap()` with proper error handling
3. Add test for edge case (retry_count is None)

---

### T3: Create CrawlType Enum
**Files**: `crates/randimg-core/src/task_queue/jobs.rs`, `handlers.rs`
**Category**: `quick`
**Skills**: []
**Verification**: `cargo test -p randimg-core` passes

**Tasks**:
1. Create `CrawlType` enum with `Ranking`, `User`, `Bookmarks` variants
2. Implement `TryFrom<i32>` for `CrawlType`
3. Replace magic numbers in `handlers.rs`
4. Add tests for CrawlType conversion

---

### T4: Remove task_dependencies Table
**Files**: `migration/src/`, `crates/randimg-core/src/db/query/task_dependency.rs`, `task_tree.rs`, `task.rs`
**Category**: `quick`
**Skills**: []
**Verification**: `cargo test -p randimg-core` passes, migration succeeds

**Tasks**:
1. Create migration `m20260605_002_drop_task_dependencies.rs` (DROP TABLE)
2. Remove `task_dependency.rs` from entities
3. Remove `task_dependency.rs` from queries
4. Update `task_tree.rs` to remove dependency references
5. Update `task.rs` to remove dependency cleanup in delete functions
6. Run tests to verify no regressions

---

## Wave 2: Core Improvements (Parallel)

### T5: Macro for Boilerplate
**Files**: `crates/randimg-core/src/task_queue/jobs.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: `cargo test -p randimg-core` passes, ~380 lines reduced to ~50

**Tasks**:
1. Analyze common pattern in all 7 `AsyncRunnable::run()` implementations
2. Create `impl_async_runnable!` macro with:
   - Status update to `running`
   - Handler call
   - Status update to `done`/`failed`
   - Error logging
   - FangError conversion
3. Replace all 7 implementations with macro invocations
4. Add tests for macro-generated code

**Pattern to extract**:
```rust
// Current pattern (repeated 7 times):
async fn run(&self, _queue: &dyn AsyncQueueable) -> Result<(), FangError> {
    let task_id = self.task_id.clone();
    // Update status to running
    if let Some(ref id) = task_id {
        let _ = query::task::update_status(id, STATUS_RUNNING).await;
    }
    // Call handler
    let result = handle_xxx(self.clone()).await;
    // Update status based on result
    match &result {
        Ok(_) => {
            if let Some(ref id) = task_id {
                let _ = query::task::update_status(id, STATUS_DONE).await;
            }
        }
        Err(e) => {
            if let Some(ref id) = task_id {
                let _ = query::task::update_status(id, STATUS_FAILED).await;
                let _ = query::task::update_error(id, e).await;
            }
        }
    }
    result.map_err(|e| FangError { description: e })
}
```

---

### T6: Batch Query Optimization
**Files**: `crates/randimg-core/src/db/query/task.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: `cargo test -p randimg-core` passes, N+1 eliminated

**Tasks**:
1. Refactor `list_roots` to use single query with JOIN or subquery
2. Use `GROUP BY` to compute child counts and derived status in-memory
3. Add test for batch query correctness
4. Benchmark old vs new (optional)

**New query pattern**:
```sql
SELECT t.*,
       COUNT(c.id) as child_count,
       COUNT(CASE WHEN c.status = 'done' THEN 1 END) as done_count,
       COUNT(CASE WHEN c.status = 'failed' THEN 1 END) as failed_count
FROM tasks t
LEFT JOIN tasks c ON c.root_id = t.id
WHERE t.parent_id IS NULL
GROUP BY t.id
```

---

### T7: Transactional Task Push
**Files**: `crates/randimg-core/src/task_queue/fang_backend.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: `cargo test -p randimg-core` passes, transaction atomicity verified

**Tasks**:
1. Wrap `push_task()` in database transaction
2. Ensure `create()` + `insert_task()` + `link_fang_task()` are atomic
3. Add rollback on failure
4. Add test for transaction failure scenario

**Implementation**:
```rust
pub async fn push_task(&self, job: impl AsyncRunnable + 'static) -> Result<String, AppError> {
    let txn = self.db.begin().await?;
    
    // Step 1: Create custom task
    let task = query::task::create(&txn, ...).await?;
    
    // Step 2: Insert into Fang
    let fang_task = self.queue.insert_task_txn(&txn, job).await?;
    
    // Step 3: Link
    query::task::link_fang_task(&txn, task.id, fang_task.id).await?;
    
    txn.commit().await?;
    Ok(task.id)
}
```

---

## Wave 3: Shutdown & Timeout (Sequential)

### T8: Graceful Shutdown
**Files**: `crates/randimg-worker/src/main.rs`, `crates/randimg-core/src/lib.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Manual test: send SIGINT, observe 30s drain before exit

**Tasks**:
1. Modify `spawn_workers()` to return `Vec<JoinHandle>` + shutdown channel
2. In `main.rs`, replace `abort()` with graceful drain:
   ```rust
   // Signal shutdown
   shutdown_tx.send(()).await;
   
   // Wait for drain timeout
   tokio::select! {
       _ = async { for h in &handles { h.await; } } => {
           info!("All workers drained gracefully");
       }
       _ = tokio::time::sleep(Duration::from_secs(30)) => {
           warn!("Drain timeout exceeded, aborting remaining tasks");
           for h in handles { h.abort(); }
       }
   }
   ```
3. Add test for graceful shutdown behavior

---

### T9: Task Timeout Mechanism
**Files**: `crates/randimg-core/src/task_queue/jobs.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Test with short timeout, verify task fails with timeout error

**Tasks**:
1. Wire `task_default_timeout_secs` from config into job struct
2. Wrap handler execution in `tokio::time::timeout()`:
   ```rust
   let timeout = Duration::from_secs(self.timeout_secs);
   let result = tokio::time::timeout(timeout, handle_xxx(self.clone())).await;
   
   match result {
       Ok(inner) => inner,
       Err(_) => Err("Task timed out".to_string()),
   }
   ```
3. Add test for timeout behavior

---

## Wave 4: Reliability Features (Parallel)

### T10: Dead Letter Queue
**Files**: `crates/randimg-core/src/db/entities/`, `crates/randimg-core/src/db/query/`, `crates/randimg-core/src/task_queue/`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Test failed task moves to DLQ after max retries

**Tasks**:
1. Create `dead_letter` entity with fields:
   - `id`, `original_task_id`, `task_type`, `error_message`, `retry_count`, `created_at`
2. Add `status = 'dead'` constant to task entity
3. Modify `AsyncRunnable::run()` to move to DLQ when `retry_count >= max_retries`
4. Add query functions for DLQ management
5. Add API endpoint to view/retry DLQ tasks
6. Add tests for DLQ flow

---

### T11: Watchdog
**Files**: `crates/randimg-worker/src/main.rs`, `crates/randimg-core/src/lib.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Kill a worker process, observe watchdog restart

**Tasks**:
1. Create `Watchdog` struct with:
   - Worker health tracking (last heartbeat per pool)
   - Periodic check interval (e.g., 30s)
   - Auto-restart on timeout
2. Integrate into worker main loop
3. Add heartbeat mechanism to worker pools
4. Add test for watchdog restart behavior

---

### T12: Health Endpoint
**Files**: `crates/randimg-worker/src/main.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: `curl http://localhost:8001/health` returns 200

**Tasks**:
1. Add Axum dependency to worker binary (with `http` feature)
2. Create minimal health router:
   ```rust
   let app = Router::new()
       .route("/health", get(health_handler))
       .route("/ready", get(ready_handler));
   ```
3. Implement `health_handler` returning worker status
4. Implement `ready_handler` checking all pools are alive
5. Start health server on separate port (configurable)
6. Add test for health endpoint

---

## Wave 5: Reliability Improvements (Parallel)

### T13: Automatic Cleanup
**Files**: `crates/randimg-core/src/task_queue/`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Old tasks deleted after configured TTL

**Tasks**:
1. Create `CleanupJob` implementing `AsyncRunnable`
2. Add config for `task_cleanup_ttl_days` (default 30)
3. Implement cleanup logic:
   ```sql
   DELETE FROM tasks 
   WHERE status IN ('done', 'failed', 'killed', 'dead')
   AND updated_at < NOW() - INTERVAL '30 days'
   ```
4. Register cleanup worker in `spawn_workers()`
5. Add test for cleanup behavior

---

### T14: Idempotency Checks
**Files**: `crates/randimg-core/src/task_queue/handlers.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Duplicate task skipped with log message

**Tasks**:
1. Add pre-execution checks to:
   - `handle_upload`: Check `is_public` flag
   - `handle_color_extract`: Check if colors already extracted
   - `handle_accessibility_check`: Check if already checked
2. Return `Ok(())` early if already done
3. Add test for idempotency

---

### T15: Auth Retry Improvement
**Files**: `crates/randimg-core/src/task_queue/handlers.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Auth error retries with exponential backoff

**Tasks**:
1. Create `retry_with_auth_recovery()` helper:
   ```rust
   async fn retry_with_auth_recovery<F, T, E>(
       max_retries: u32,
       base_delay: Duration,
       f: F,
   ) -> Result<T, E>
   where F: Fn() -> Future<Output = Result<T, E>> + Send
   {
       for attempt in 0..max_retries {
           match f().await {
               Ok(val) => return Ok(val),
               Err(e) if is_auth_error(&e) && attempt < max_retries - 1 => {
                   recover_auth().await?;
                   let delay = base_delay * 2u32.pow(attempt);
                   tokio::time::sleep(delay).await;
               }
               Err(e) => return Err(e),
           }
       }
       unreachable!()
   }
   ```
2. Replace single-retry pattern in all handlers
3. Add test for retry behavior

---

### T16: Discover Deduplication
**Files**: `crates/randimg-core/src/task_queue/handlers.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Duplicate illust_id skipped with log message

**Tasks**:
1. Add `DashMap<Uuid, Instant>` to WorkerState for dedup cache
2. Check cache before processing discover results
3. Add TTL for cache entries (e.g., 1 hour)
4. Add test for deduplication

---

## Wave 6: New Features (Parallel)

### T17: Task Priority
**Files**: `migration/src/`, `crates/randimg-core/src/db/entities/task.rs`, `task_queue/`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: High-priority task executed before low-priority

**Tasks**:
1. Add `priority` column to tasks table (INTEGER, default 0)
2. Create migration
3. Update entity and queries
4. Modify Fang task insertion to use priority
5. Add API endpoint to set priority
6. Add test for priority ordering

---

### T18: Task Progress Tracking
**Files**: `crates/randimg-core/src/db/entities/task.rs`, `task_queue/handlers.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Progress percentage visible in task list

**Tasks**:
1. Add `progress` column to tasks table (FLOAT, default 0.0)
2. Create migration
3. Update entity and queries
4. Add progress reporting to long-running handlers (crawl, download)
5. Add API endpoint to query progress
6. Add test for progress updates

---

### T19: Task Deduplication
**Files**: `crates/randimg-core/src/task_queue/fang_backend.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Duplicate task rejected with error

**Tasks**:
1. Create `TaskFingerprint` from job params hash
2. Add `DashMap<TaskFingerprint, Instant>` to QueueBackend
3. Check before `push_task()`
4. Add config for dedup TTL
5. Add test for deduplication

---

### T20: Task Metrics
**Files**: `crates/randimg-core/src/task_queue/`, `crates/randimg-worker/src/main.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Metrics endpoint returns statistics

**Tasks**:
1. Create `TaskMetrics` struct:
   ```rust
   struct TaskMetrics {
       total_executed: AtomicU64,
       total_succeeded: AtomicU64,
       total_failed: AtomicU64,
       avg_duration_ms: AtomicU64,
   }
   ```
2. Increment counters in `AsyncRunnable::run()`
3. Expose via `/metrics` endpoint
4. Add test for metrics collection

---

## Wave 7: Schema Improvements (Sequential)

### T21: JSONB Migration
**Files**: `migration/src/`, `crates/randimg-core/src/db/query/`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Queries use native JSON operators

**Tasks**:
1. Create migration to alter `params` column from TEXT to JSONB
2. Update all queries using `::json` casts
3. Add GIN index on `params` for JSON queries
4. Test all JSON queries still work

---

### T22: Enum Constraints
**Files**: `migration/src/`, `crates/randimg-core/src/db/entities/task.rs`
**Category**: `unspecified-high`
**Skills**: []
**Verification**: Invalid status/task_type rejected by DB

**Tasks**:
1. Create PostgreSQL ENUM types:
   ```sql
   CREATE TYPE task_status AS ENUM ('pending', 'queued', 'running', 'done', 'failed', 'killed', 'dead');
   CREATE TYPE task_type AS ENUM ('crawl', 'download', 'color_extract', 'upload', 'accessibility_check', 'discover', 'refresh_pixiv_token');
   ```
2. Create migration to alter columns
3. Update entity types
4. Test invalid values rejected

---

## Risk Assessment

| Task | Risk | Impact | Skip if time-constrained? |
|------|------|--------|---------------------------|
| T1: Indexes | Low | High | No |
| T2: Fix unwrap | Low | Low | Yes |
| T3: CrawlType enum | Low | Low | Yes |
| T4: Remove table | Medium | Medium | No |
| T5: Macro | High | High | No |
| T6: Batch queries | Medium | High | No |
| T7: Transactional push | High | Critical | No |
| T8: Graceful shutdown | High | Critical | No |
| T9: Task timeout | Medium | High | No |
| T10: DLQ | High | High | No |
| T11: Watchdog | Medium | Medium | Yes |
| T12: Health endpoint | Low | Medium | No |
| T13: Auto cleanup | Low | Medium | No |
| T14: Idempotency | Medium | Medium | Yes |
| T15: Auth retry | Low | Medium | No |
| T16: Discover dedup | Low | Low | Yes |
| T17: Priority | Medium | Medium | Yes |
| T18: Progress tracking | Medium | Medium | Yes |
| T19: Task dedup | Medium | Medium | Yes |
| T20: Metrics | Low | Low | Yes |
| T21: JSONB migration | High | Medium | Yes |
| T22: Enum constraints | Medium | Low | Yes |

---

## Verification Criteria

Each task must pass:
1. **Unit tests**: `cargo test -p randimg-core` passes
2. **Integration test**: `cargo test -p randimg-core -- --ignored` passes (if DB tests exist)
3. **Build**: `cargo build` succeeds
4. **LSP**: No diagnostics errors on changed files
5. **Manual verification**: For critical tasks (T7, T8, T9, T10, T12)

---

## Execution Order

**Immediate (Wave 1)**: T1, T2, T3, T4 in parallel
**After Wave 1**: T5, T6, T7 in parallel
**After Wave 2**: T8, then T9 sequentially
**After Wave 3**: T10, T11, T12 in parallel
**After Wave 4**: T13, T14, T15, T16 in parallel
**After Wave 5**: T17, T18, T19, T20 in parallel
**After Wave 6**: T21, then T22 sequentially

**Total estimated time**: 8-12 hours with parallel execution
