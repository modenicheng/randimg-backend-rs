# Flatten Task Tree API Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `?flatten=true` query parameter to `GET /tasks/{id}/tree` to return a flat list with parent/root context instead of nested tree.

**Architecture:** Extend existing `get_task_tree` handler with query parameter extraction. When `flatten=true`, recursively collect all descendant jobs into a flat array, adding `parent_job_id` and `root_job_id` to each item. No changes to SQL layer — flatten in Rust post-processing.

**Tech Stack:** Rust, Axum, SeaORM, serde

---

## API Schema Comparison

### Current: `GET /tasks/{id}/tree` (nested tree)

**Response:**
```json
{
  "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD",
  "children": [
    {
      "job": {
        "id": "01KT4B9R5G5CKP40MMBN0EG4SE",
        "jobType": "discover",
        "status": "completed",
        "rawStatus": "Done",
        "attempts": 1,
        "maxAttempts": 4,
        "priority": 0,
        "runAt": "2026-06-01T12:00:00Z",
        "doneAt": "2026-06-01T12:05:00Z",
        "lastResult": null,
        "payload": { "illust_id": "12345", "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD" }
      },
      "children": [
        {
          "job": {
            "id": "01KT4BCNQKBSJX0VM5B2S4TCTQ",
            "jobType": "discover",
            "status": "completed",
            "rawStatus": "Done",
            "attempts": 1,
            "maxAttempts": 4,
            "priority": 0,
            "runAt": "2026-06-01T12:05:00Z",
            "doneAt": "2026-06-01T12:10:00Z",
            "lastResult": null,
            "payload": { "illust_id": "67890", "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD" }
          },
          "children": [
            {
              "job": {
                "id": "01KT4BHBNWQ68F6A0WS8BES7Q7",
                "jobType": "download",
                "status": "completed",
                "rawStatus": "Done",
                "attempts": 1,
                "maxAttempts": 4,
                "priority": 0,
                "runAt": "2026-06-01T12:10:00Z",
                "doneAt": "2026-06-01T12:15:00Z",
                "lastResult": null,
                "payload": { "image_id": "img_001", "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD" }
              },
              "children": []
            }
          ]
        }
      ]
    }
  ]
}
```

**Structure:** Nested tree via `children` arrays. Each node has `job` (the task data) and `children` (nested descendants). Depth information is implicit in nesting level.

---

### Proposed: `GET /tasks/{id}/tree?flatten=true` (flat list)

**Response:**
```json
{
  "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD",
  "tasks": [
    {
      "id": "01KT4B9R5G5CKP40MMBN0EG4SE",
      "jobType": "discover",
      "status": "completed",
      "rawStatus": "Done",
      "attempts": 1,
      "maxAttempts": 4,
      "priority": 0,
      "runAt": "2026-06-01T12:00:00Z",
      "doneAt": "2026-06-01T12:05:00Z",
      "lastResult": null,
      "payload": { "illust_id": "12345", "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD" },
      "parent_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD",
      "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD"
    },
    {
      "id": "01KT4BCNQKBSJX0VM5B2S4TCTQ",
      "jobType": "discover",
      "status": "completed",
      "rawStatus": "Done",
      "attempts": 1,
      "maxAttempts": 4,
      "priority": 0,
      "runAt": "2026-06-01T12:05:00Z",
      "doneAt": "2026-06-01T12:10:00Z",
      "lastResult": null,
      "payload": { "illust_id": "67890", "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD" },
      "parent_job_id": "01KT4B9R5G5CKP40MMBN0EG4SE",
      "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD"
    },
    {
      "id": "01KT4BHBNWQ68F6A0WS8BES7Q7",
      "jobType": "download",
      "status": "completed",
      "rawStatus": "Done",
      "attempts": 1,
      "maxAttempts": 4,
      "priority": 0,
      "runAt": "2026-06-01T12:10:00Z",
      "doneAt": "2026-06-01T12:15:00Z",
      "lastResult": null,
      "payload": { "image_id": "img_001", "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD" },
      "parent_job_id": "01KT4BCNQKBSJX0VM5B2S4TCTQ",
      "root_job_id": "01KT4B9M8V1MKZKH1291VQ3HXD"
    }
  ]
}
```

**Structure:** Flat array `tasks`. Each item is the job data (same fields as `model_to_json` output) plus two new fields:
- `parent_job_id`: The direct parent's job ID (null for root's direct children if root has no parent)
- `root_job_id`: The root job ID passed in the URL path

---

### Key Differences

| Aspect | Nested (default) | Flat (`?flatten=true`) |
|--------|------------------|------------------------|
| Response key | `children` | `tasks` |
| Structure | Recursive `children` arrays | Single flat array |
| Depth info | Implicit (nesting level) | Explicit via `parent_job_id` chain |
| Parent ref | Implicit (parent node contains `children`) | Explicit `parent_job_id` field |
| Root ref | Implicit (top-level `root_job_id`) | Explicit `root_job_id` field per item |
| Frontend use | Tree UI, expand/collapse | Table/list UI, sorting, filtering |

---

## Task 1: Add `TaskTreeQuery` struct

**Files:**
- Modify: `src/handlers/task.rs:616-635`

- [ ] **Step 1: Define query params struct**

```rust
#[derive(Debug, Deserialize)]
pub struct TaskTreeQuery {
    pub flatten: Option<bool>,
}
```

- [ ] **Step 2: Update `get_task_tree` signature**

```rust
pub async fn get_task_tree(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
    Query(q): Query<TaskTreeQuery>,
) -> Result<Json<serde_json::Value>, AppError>
```

- [ ] **Step 3: Commit**

```bash
git add src/handlers/task.rs
git commit -m "feat: add TaskTreeQuery struct for flatten parameter"
```

---

## Task 2: Implement flatten logic

**Files:**
- Modify: `src/handlers/task.rs:616-635`

- [ ] **Step 1: Add recursive flatten helper**

```rust
fn flatten_tree(nodes: &[ChildJobNode], parent_id: &str, root_id: &str) -> Vec<serde_json::Value> {
    let mut result = Vec::new();
    for node in nodes {
        let mut job = node.job.clone();
        // Add parent_job_id and root_job_id to each job
        if let serde_json::Value::Object(ref mut map) = job {
            map.insert("parent_job_id".to_string(), serde_json::Value::String(parent_id.to_string()));
            map.insert("root_job_id".to_string(), serde_json::Value::String(root_id.to_string()));
        }
        result.push(job);
        // Recursively flatten children
        let node_id = node.job.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        result.extend(flatten_tree(&node.children, node_id, root_id));
    }
    result
}
```

- [ ] **Step 2: Use flatten when query param is true**

```rust
pub async fn get_task_tree(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(task_id): Path<String>,
    Query(q): Query<TaskTreeQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tree = query::task_tree::list_children(&state.db, &task_id, None, None, 20).await?;

    if q.flatten.unwrap_or(false) {
        let tasks = flatten_tree(&tree, &task_id, &task_id);
        Ok(Json(serde_json::json!({
            "root_job_id": task_id,
            "tasks": tasks,
        })))
    } else {
        Ok(Json(serde_json::json!({
            "root_job_id": task_id,
            "children": tree,
        })))
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add src/handlers/task.rs
git commit -m "feat: implement flatten logic for task tree endpoint"
```

---

## Task 3: Add test for flatten behavior

**Files:**
- Modify: `tests/api_test.rs` (or relevant test file)

- [ ] **Step 1: Add test case**

```rust
#[tokio::test]
async fn test_task_tree_flatten() {
    // Assumes a running server with seeded test data
    let client = reqwest::Client::new();
    let base = "http://localhost:8000";

    // Get nested tree
    let nested: serde_json::Value = client
        .get(format!("{}/tasks/{}/tree", base, ROOT_JOB_ID))
        .send().await.unwrap().json().await.unwrap();

    // Get flattened tree
    let flat: serde_json::Value = client
        .get(format!("{}/tasks/{}/tree?flatten=true", base, ROOT_JOB_ID))
        .send().await.unwrap().json().await.unwrap();

    // Both should have root_job_id
    assert!(nested.get("root_job_id").is_some());
    assert!(flat.get("root_job_id").is_some());

    // Nested has "children", flat has "tasks"
    assert!(nested.get("children").is_some());
    assert!(flat.get("tasks").is_some());

    // Flat tasks should have parent_job_id and root_job_id
    let tasks = flat["tasks"].as_array().unwrap();
    for task in tasks {
        assert!(task.get("parent_job_id").is_some(), "missing parent_job_id");
        assert!(task.get("root_job_id").is_some(), "missing root_job_id");
        assert_eq!(task["root_job_id"].as_str().unwrap(), ROOT_JOB_ID);
    }
}
```

- [ ] **Step 2: Run test**

Run: `cargo test test_task_tree_flatten -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/api_test.rs
git commit -m "test: add test for flatten task tree endpoint"
```

---

## Task 4: Update OpenAPI docs (if applicable)

**Files:**
- Check if there's an OpenAPI spec file (e.g., `openapi.yaml` or auto-generated docs)

- [ ] **Step 1: Document the new query parameter**

Add to the `GET /tasks/{id}/tree` endpoint spec:
```yaml
parameters:
  - name: flatten
    in: query
    required: false
    schema:
      type: boolean
      default: false
    description: If true, return flat list with parent_job_id and root_job_id instead of nested tree
```

- [ ] **Step 2: Commit**

```bash
git add docs/
git commit -m "docs: document flatten query parameter for task tree endpoint"
```

---

## Task 5: Add detailed documentation comments

**Purpose:** The `?flatten=true` feature changes the response shape significantly. Without clear comments, reviewers might think the different response format is a bug and "fix" it back to nested. Add inline comments explaining the design intent.

**Files:**
- Modify: `src/handlers/task.rs` (wherever Task 1-2 code lands)

- [ ] **Step 1: Add module-level or function-level doc comment on `get_task_tree`**

```rust
/// GET /tasks/:id/tree
///
/// Returns the task tree rooted at `id`.
///
/// # Query Parameters
///
/// - `flatten` (optional, default: `false`)
///   - When `false` (default): Returns nested tree structure with `children` arrays.
///     Each node has `{ job: {...}, children: [...] }`. Frontend uses this for
///     tree UI with expand/collapse.
///   - When `true`: Returns flat list in `tasks` array. Each item includes
///     `parent_job_id` (direct parent) and `root_job_id` (the URL path root).
///     Frontend uses this for table/list UI with sorting and filtering.
///
/// # Design Decision: Why flatten?
///
/// The nested tree format is natural for tree UI but painful for:
/// - Sorting/filtering across all descendants (requires recursive traversal in JS)
/// - Status rollup queries (need to flatten first anyway)
/// - Table display with columns (nested JSON doesn't map to rows)
///
/// The flat format trades tree structure for O(1) access to any task by index.
/// `parent_job_id` and `root_job_id` preserve the hierarchy information so the
/// frontend can reconstruct the tree if needed (build adjacency list from
/// parent_job_id).
///
/// # Why not a separate endpoint?
///
/// `?flatten` is a query parameter (not `/tasks/:id/tree/flat`) because:
/// - Same underlying data, different serialization
/// - Single route to maintain
/// - Consistent with REST conventions (representation varies by query params)
pub async fn get_task_tree(
```

- [ ] **Step 2: Add inline comment on `flatten_tree` helper**

```rust
/// Recursively flattens a `ChildJobNode` tree into a flat `Vec<JsonValue>`.
///
/// For each node, this function:
/// 1. Clones the job data
/// 2. Injects `parent_job_id` and `root_job_id` fields
/// 3. Pushes to the result vector
/// 4. Recursively processes children (depth-first, pre-order traversal)
///
/// The root node itself is NOT included — only its descendants. This matches
/// the nested mode behavior where the root is the URL parameter, not a child.
///
/// # Arguments
/// - `nodes`: The child nodes to flatten (output of `list_children`)
/// - `parent_id`: The job ID of the parent for the current level
/// - `root_id`: The root job ID (constant throughout recursion, from URL path)
fn flatten_tree(nodes: &[ChildJobNode], parent_id: &str, root_id: &str) -> Vec<serde_json::Value> {
```

- [ ] **Step 3: Add comment on the response shape branching**

```rust
    if q.flatten.unwrap_or(false) {
        // FLATTENED MODE: Return all descendants as a flat array.
        // Each item has parent_job_id and root_job_id for hierarchy reconstruction.
        // Response shape: { "root_job_id": "...", "tasks": [...] }
        //
        // This differs from the default nested mode which returns:
        // { "root_job_id": "...", "children": [{ job, children }] }
        //
        // The different shapes are intentional — NOT a bug. See function doc.
        let tasks = flatten_tree(&tree, &task_id, &task_id);
        Ok(Json(serde_json::json!({
            "root_job_id": task_id,
            "tasks": tasks,
        })))
    } else {
        // NESTED MODE: Return tree structure with children arrays.
        // Each node has { job: {...}, children: [...] } recursively.
        Ok(Json(serde_json::json!({
            "root_job_id": task_id,
            "children": tree,
        })))
    }
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`
Expected: No errors (comments don't affect compilation)

- [ ] **Step 5: Commit**

```bash
git add src/handlers/task.rs
git commit -m "docs: add detailed comments explaining flatten design decision"
```

---

## Notes

- **camelCase vs snake_case inconsistency**: The existing `model_to_json` uses camelCase (`jobType`, `rawStatus`) while `row_to_json` uses snake_case. This plan preserves the existing behavior — the new `parent_job_id` and `root_job_id` fields use snake_case to match `model_to_json`'s pattern of using camelCase for the original fields but snake_case for new additions. If consistency is desired, a separate refactoring task should unify them.

- **Performance**: The flatten operation is O(n) where n = total descendants. No additional DB queries — it's pure in-memory traversal of the already-fetched tree.

- **Root node**: The root job itself is NOT included in the flat list (only descendants). The `root_job_id` field on each item points back to the root.
