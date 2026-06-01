# Cancel All Pending Tasks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Cancel All" button to the task management page that deletes all pending tasks at once, with optional filtering by task type.

**Architecture:** Backend adds a `DELETE /tasks/pending` endpoint (with optional `task_type` query filter) that bulk-deletes pending tasks via SeaORM. Frontend adds a confirmation dialog and button to the task manager header. This follows the existing pattern where "cancel" means delete (same as the per-task cancel).

**Tech Stack:** Rust, SeaORM, Axum (backend); Vue 3, Vuetify 3, Axios (frontend)

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/db/query/apalis_job.rs` | Modify | Add `delete_pending()` query function |
| `src/handlers/task.rs` | Modify | Add `DELETE /tasks/pending` route + handler |
| `~/coding/randimg-frontend/src/views/TaskManager.vue` | Modify | Add "Cancel All" button + confirmation dialog |

---

### Task 1: Add `delete_pending` query function (backend)

**Files:**
- Modify: `/home/modenicheng/coding/randimg-backend-rs/src/db/query/apalis_job.rs`

- [ ] **Step 1: Add `delete_pending` function**

Append this function to the bottom of `src/db/query/apalis_job.rs`:

```rust
/// Delete all pending jobs, optionally filtered by job type.
/// Returns the number of rows deleted.
pub async fn delete_pending(
    db: &DatabaseConnection,
    task_type: Option<&str>,
) -> Result<u64, DbErr> {
    let mut delete = ApalisJob::delete_many()
        .filter(apalis_job::Column::Status.eq(apalis_job::STATUS_PENDING));
    if let Some(tt) = task_type {
        delete = delete.filter(apalis_job::Column::JobType.eq(tt));
    }
    let result = delete.exec(db).await?;
    Ok(result.rows_affected)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`
Expected: no errors (may show warnings)

- [ ] **Step 3: Commit**

```bash
git add src/db/query/apalis_job.rs
git commit -m "feat: add delete_pending query for bulk task cancellation"
```

---

### Task 2: Add `DELETE /tasks/pending` endpoint (backend)

**Files:**
- Modify: `/home/modenicheng/coding/randimg-backend-rs/src/handlers/task.rs`

- [ ] **Step 1: Update route registration**

In `src/handlers/task.rs`, change the `routes()` function from:

```rust
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks", get(list_tasks))
        .route("/tasks/{task_id}", get(get_task).delete(delete_task))
}
```

to:

```rust
use axum::routing::delete;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks", get(list_tasks))
        .route("/tasks/pending", delete(delete_pending_tasks))
        .route("/tasks/{task_id}", get(get_task).delete(delete_task))
}
```

Note: `delete` routing function must be added to the existing `use axum::{...}` import. Add `routing::delete` alongside the existing `routing::get`.

- [ ] **Step 2: Add query struct and handler**

Add this query struct and handler function after the existing `delete_task` handler:

```rust
#[derive(Deserialize)]
pub struct DeletePendingQuery {
    pub task_type: Option<String>,
}

/// DELETE /tasks/pending — Delete all pending tasks, optionally filtered by type
pub async fn delete_pending_tasks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<DeletePendingQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = query::apalis_job::delete_pending(&state.db, q.task_type.as_deref())
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "message": "Pending tasks deleted",
        "deleted": deleted,
    })))
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add src/handlers/task.rs
git commit -m "feat: add DELETE /tasks/pending endpoint for bulk cancellation"
```

---

### Task 3: Add "Cancel All" button to frontend

**Files:**
- Modify: `/home/modenicheng/coding/randimg-frontend/src/views/TaskManager.vue`

- [ ] **Step 1: Add cancel-all state variables**

After the existing `const creating = ref(false);` line (around line 30), add:

```typescript
const cancelAllDialog = ref(false);
const cancellingAll = ref(false);
```

- [ ] **Step 2: Add `cancelAllTasks` function**

After the existing `retryTask` function (around line 176), add:

```typescript
const cancelAllTasks = async () => {
  cancellingAll.value = true;
  try {
    const params: Record<string, any> = {};
    if (filterType.value) params.task_type = filterType.value;
    const res = await Axios.delete('/tasks/pending', { params });
    cancelAllDialog.value = false;
    snackbar.value = {
      show: true,
      text: `已取消 ${res.data.deleted ?? 0} 个任务`,
      color: 'success',
    };
    await fetchTasks();
  } catch (e: any) {
    showError(e.response?.data?.message ?? '取消失败');
  } finally {
    cancellingAll.value = false;
  }
};
```

- [ ] **Step 3: Add "Cancel All" button to header**

Change the header section (around lines 210-217) from:

```html
<v-col cols="auto">
  <v-btn color="primary" @click="createDialog = true">创建任务</v-btn>
</v-col>
```

to:

```html
<v-col cols="auto" class="d-flex ga-2">
  <v-btn color="error" variant="outlined" @click="cancelAllDialog = true">取消所有任务</v-btn>
  <v-btn color="primary" @click="createDialog = true">创建任务</v-btn>
</v-col>
```

- [ ] **Step 4: Add confirmation dialog**

Before the existing `<!-- Snackbar -->` section (around line 345), add:

```html
<!-- Cancel All Confirmation Dialog -->
<v-dialog v-model="cancelAllDialog" max-width="420">
  <v-card>
    <v-card-title>确认取消所有任务</v-card-title>
    <v-card-text>
      此操作将删除所有待处理的任务{{ filterType ? `（类型: ${jobLabel[filterType] ?? filterType}）` : '' }}，不可撤销。确定继续吗？
    </v-card-text>
    <v-card-actions>
      <v-spacer />
      <v-btn text="返回" @click="cancelAllDialog = false" />
      <v-btn color="error" text="确认取消" :loading="cancellingAll" @click="cancelAllTasks" />
    </v-card-actions>
  </v-card>
</v-dialog>
```

- [ ] **Step 5: Verify frontend builds**

Run: `cd ~/coding/randimg-frontend && npx vue-tsc --noEmit 2>&1 | tail -10`
Expected: no errors (may show unrelated warnings)

- [ ] **Step 6: Commit**

```bash
cd ~/coding/randimg-frontend
git add src/views/TaskManager.vue
git commit -m "feat: add cancel-all-pending-tasks button to task manager"
```

---

## Verification

After implementing all tasks:

1. **Backend:** `cargo check` — no errors
2. **Frontend:** `cd ~/coding/randimg-frontend && npx vue-tsc --noEmit` — no errors
3. **Manual test:** Start the backend, navigate to `/tasks`, create some crawl tasks, click "取消所有任务", confirm — all pending tasks should be deleted and the count shown in the snackbar.
4. **Filtered test:** Set the type filter to "下载", click "取消所有任务" — only pending download tasks should be deleted.
