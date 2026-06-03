# Flatten Pagination Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add server-side pagination to `GET /tasks/{id}/tree?flatten=true` to prevent browser crashes on large task lists.

**Architecture:** Add `limit` and `offset` query parameters to the existing `TaskTreeQuery` struct. When `flatten=true`, paginate the flattened result and return `total` count. Frontend reuses existing pagination component pattern.

**Tech Stack:** Rust, Axum, SeaORM, Vue 3, Vuetify 3

---

## Files to Modify

| File | Change |
|------|--------|
| `src/handlers/task.rs:650-725` | Add pagination params to `TaskTreeQuery`, paginate flattened result |
| `../randimg-frontend/src/views/TaskManager.vue:21-26, 259-280` | Add pagination state back to `SubtaskState`, use server-side pagination |

---

## Task 1: Add Pagination to Backend Flatten Endpoint

**Files:**
- Modify: `src/handlers/task.rs:650-725`
- Test: `tests/api_test.rs` (existing `test_task_tree_flatten`)

### Step 1: Update `TaskTreeQuery` struct

Add `limit` and `offset` fields to the query struct:

```rust
#[derive(Debug, Deserialize)]
pub struct TaskTreeQuery {
    pub flatten: Option<bool>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}
```

### Step 2: Update handler to paginate flattened result

Modify `get_task_tree` to slice the flattened array when pagination params are present:

```rust
pub async fn get_task_tree(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
    Query(q): Query<TaskTreeQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tree = query::task_tree::list_children(
        &state.db,
        &task_id,
        None,
        None,
        20,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    if q.flatten.unwrap_or(false) {
        let all_tasks = flatten_tree(&tree, &task_id, &task_id);
        let total = all_tasks.len() as u64;
        
        // Apply pagination if limit/offset provided
        let offset = q.offset.unwrap_or(0) as usize;
        let limit = q.limit.unwrap_or(total) as usize;
        let tasks: Vec<_> = all_tasks
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect();
        
        Ok(Json(serde_json::json!({
            "root_job_id": task_id,
            "tasks": tasks,
            "total": total,
        })))
    } else {
        Ok(Json(serde_json::json!({
            "root_job_id": task_id,
            "children": tree,
        })))
    }
}
```

### Step 3: Update documentation

Update the doc comment for `get_task_tree` to document the new pagination parameters:

```rust
/// # Query Parameters
///
/// - `flatten` (optional, default: `false`)
///   - When `false` (default): Returns nested tree structure with `children` arrays.
///   - When `true`: Returns flat list in `tasks` array with pagination support.
///
/// - `limit` (optional): Maximum number of tasks to return in flattened mode.
///   Defaults to all tasks if not specified.
///
/// - `offset` (optional): Number of tasks to skip in flattened mode.
///   Defaults to 0 if not specified.
```

### Step 4: Test pagination behavior

Run the existing test and verify it still passes:

```bash
cargo test test_task_tree_flatten -- --nocapture
```

Expected: Test passes (it doesn't send `limit`/`offset`, so it gets all tasks).

### Step 5: Commit

```bash
git add src/handlers/task.rs
git commit -m "feat: add limit/offset pagination to flatten tree endpoint"
```

---

## Task 2: Update Frontend to Use Server-Side Pagination

**Files:**
- Modify: `../randimg-frontend/src/views/TaskManager.vue`

### Step 1: Add pagination state to SubtaskState

Update the interface and initialization to include pagination:

```typescript
interface SubtaskState {
  items: Task[];
  loading: boolean;
  loaded: boolean;
  filterType: string | null;
  // Pagination
  total: number;
  page: number;
  pageSize: number;
}
```

### Step 2: Update `onRootExpandChange` initialization

```typescript
subtaskMap[id] = {
  items: [],
  loading: false,
  loaded: false,
  filterType: null,
  total: 0,
  page: 1,
  pageSize: 50,
};
```

### Step 3: Update `fetchSubtasks` to use pagination

```typescript
const fetchSubtasks = async (rootId: string, silent = false) => {
  const state = subtaskMap[rootId];
  if (!state) return;
  if (!silent) state.loading = true;
  try {
    const offset = (state.page - 1) * state.pageSize;
    const res = await Axios.get(`/tasks/${rootId}/tree`, {
      params: {
        flatten: true,
        limit: state.pageSize,
        offset: offset,
      }
    });
    if (res.status === 200) {
      let tasks = (res.data.tasks ?? []).map(parseTask);
      state.total = res.data.total ?? 0;
      // Apply client-side type filter if set
      if (state.filterType) {
        tasks = tasks.filter((t: Task) => t.jobType === state.filterType);
      }
      state.items = tasks;
      state.loaded = true;
    }
  } catch (e: any) {
    showError(e.response?.data?.message ?? '子任务加载失败');
  } finally {
    state.loading = false;
  }
};
```

### Step 4: Add page change handler

```typescript
const onSubtaskPageChange = (rootId: string, newPage: number) => {
  const state = subtaskMap[rootId];
  if (!state) return;
  state.page = newPage;
  fetchSubtasks(rootId);
};
```

### Step 5: Add pagination component to template

Add after the subtask list (around line 850):

```vue
<v-pagination
  v-if="subtaskMap[root.id] && subtaskMap[root.id].total > subtaskMap[root.id].pageSize"
  v-model="subtaskMap[root.id].page"
  :length="Math.ceil(subtaskMap[root.id].total / subtaskMap[root.id].pageSize)"
  rounded="circle"
  density="compact"
  class="mt-4"
  @update:model-value="onSubtaskPageChange(root.id, $event)"
/>
```

### Step 6: Update total count display

Change line 742 from:
```vue
共 {{ subtaskMap[root.id].items.length }} 条
```
To:
```vue
共 {{ subtaskMap[root.id].total }} 条
```

### Step 7: Build and verify

```bash
cd ../randimg-frontend && npm run build
```

Expected: Build succeeds.

### Step 8: Commit

```bash
git add src/views/TaskManager.vue
git commit -m "feat: use server-side pagination for flatten tree"
```

---

## API Response Format

**Before (no pagination):**
```json
{
  "root_job_id": "01HZ...",
  "tasks": [...1000 items...]
}
```

**After (with pagination):**
```json
{
  "root_job_id": "01HZ...",
  "tasks": [...50 items...],
  "total": 1000
}
```

**Query parameters:**
- `?flatten=true` — Enable flat mode
- `?flatten=true&limit=50&offset=0` — First page, 50 items
- `?flatten=true&limit=50&offset=50` — Second page

---

## Self-Review Checklist

- [x] **Spec coverage:** Backend pagination params ✓, frontend pagination UI ✓
- [x] **No placeholders:** All code blocks are complete
- [x] **Type consistency:** `TaskTreeQuery` fields match between tasks
- [x] **Backward compatible:** No `limit`/`offset` = return all (existing behavior)