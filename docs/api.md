# Randimg API Reference

Base URL: `http://localhost:8765` (configurable via `SERVER_ADDR` env var)

All timestamps are in UTC+8 (Asia/Shanghai) format: `YYYY-MM-DD HH:MM:SS`.

---

## Authentication

Admin endpoints require a JWT token in the `Authorization` header:

```
Authorization: Bearer <token>
```

### Obtain a token

```
POST /token
Content-Type: application/json

{ "username": "admin", "password": "..." }
```

Response:

```json
{ "token": "eyJhbGciOi..." }
```

Returns `401` on invalid credentials.

---

## Public Endpoints

### Health Check

```
GET /health
```

Response: `200 OK`

### Random Image

```
GET /?format=json&ratio=landscape&width=800&tags=nature
```

| Param    | Type   | Description |
|----------|--------|-------------|
| `format` | string | `json` (default) or `redirect` |
| `local`  | bool   | Return local file path instead of CDN URL |
| `ratio`  | string | `landscape`, `portrait`, `square` |
| `width`  | int    | Minimum width filter |
| `height` | int    | Minimum height filter |
| `author` | string | Filter by author name |
| `tags`   | string | Comma-separated tag filter |

### Image Detail

```
GET /image/{id}
```

### Paginated Image List

```
GET /list?offset=0&limit=40&accessible=true&desc=true&tags=nature&author=pixiv_user
```

| Param        | Type   | Default | Description |
|--------------|--------|---------|-------------|
| `offset`     | int    | 0       | Pagination offset |
| `limit`      | int    | 30      | Page size (max 300) |
| `desc`       | bool   | true    | Sort descending |
| `sort_by`    | string | `id`    | Sort field: `id`, `width`, `height`, `aspect_ratio`, `source_created_at`, `created_at`, `popularity` |
| `ratio_floor`| float  | 0       | Minimum aspect ratio |
| `ratio_ceil` | float  | 10      | Maximum aspect ratio |
| `author`     | string | —       | Filter by author name |
| `tags`       | string | —       | Comma-separated tag filter |
| `accessible` | string | —       | **See note below** |

**`accessible` field semantics:**

- `accessible` is a boolean field on the `image` table controlling **non-admin user visibility**.
- **Non-admin users**: The API always filters `accessible != false`. Images with `accessible = false` are excluded. Non-admin users never see inaccessible images regardless of the query parameter.
- **Admin users** (authenticated with valid token): All images are returned by default (no accessible filter applied). The `accessible` query parameter allows admin to narrow results:
  - `?accessible=true` — only images where `accessible = true`
  - `?accessible=false` — only images where `accessible = false`
  - Parameter omitted or unrecognized value — all images (no filter)
- In summary: `accessible` controls **non-admin visibility**, not admin visibility. Admin sees everything by default.

### Color Search

```
GET /color/search?hex=FF5733&mode=lab&max_dist=30&limit=20
```

| Param      | Type   | Default | Description |
|------------|--------|---------|-------------|
| `hex`      | string | —       | Target color as 6-digit hex (without `#`) |
| `mode`     | string | `lab`   | `lab` (CIELAB Euclidean) or `rgb` (Euclidean in sRGB) |
| `max_dist` | float  | 30      | Maximum color distance (lower = stricter match) |
| `limit`    | int    | 20      | Max results |

### Statistics

```
GET /statistic
```

### Tags

```
GET /tags
```

### Authors

```
GET /authors
GET /authors/{id}
```

---

## Admin Endpoints (Auth Required)

### Image Management

| Method   | Path           | Description |
|----------|----------------|-------------|
| `PATCH`  | `/image/{id}`  | Update image metadata |
| `DELETE` | `/image/{id}`  | Soft-delete image |

### Tag Management

| Method   | Path         | Description |
|----------|--------------|-------------|
| `PATCH`  | `/tags/{id}` | Update tag |
| `DELETE` | `/tags/{id}` | Delete tag |

### Crawler

| Method | Path               | Description |
|--------|--------------------|-------------|
| `GET`  | `/crawler`         | List crawler tasks |
| `POST` | `/crawler`         | Create crawler task |
| `POST` | `/crawler/discover`| Trigger autonomous discovery |

### Pixiv Credentials

| Method | Path                          | Description |
|--------|-------------------------------|-------------|
| `GET`  | `/pixiv-credential`           | List credentials |
| `POST` | `/pixiv-credential`           | Add credential |
| `GET`  | `/pixiv-credential/{id}`      | Get credential |
| `PATCH`| `/pixiv-credential/{id}`      | Update credential |
| `DELETE`| `/pixiv-credential/{id}`     | Delete credential |
| `POST` | `/pixiv-credential/{id}/refresh` | Submit token refresh task |

---

## Task Management Endpoints

All task endpoints require authentication.

### Common Response Fields

Every task object returned by the API has these fields:

| Field         | Type    | Description |
|---------------|---------|-------------|
| `id`          | string  | ULID task identifier |
| `job_type`    | string  | `crawl`, `download`, `color_extract`, `upload`, `accessibility_check`, `discover`, `refresh_pixiv_token` |
| `status`      | string  | `pending`, `running`, `completed`, `failed` |
| `priority`    | int     | Task priority (higher = more urgent) |
| `attempts`    | int     | How many times this task has been attempted |
| `max_attempts`| int     | Maximum attempts before the task is considered failed (default: 4 = 1 initial + 3 retries) |
| `run_at`      | string  | When the task was scheduled to run (UTC+8) |
| `done_at`     | string? | When the task finished (null if not done) |
| `last_result` | string? | Error message from the last failed attempt |
| `payload`     | object? | Deserialized job parameters (varies by `job_type`) |

Status mapping from Apalis internals to API:

| Apalis Status | API Status   | Meaning |
|---------------|--------------|---------|
| `Pending`     | `pending`    | Waiting for a worker to pick it up |
| `Running`     | `running`    | Currently being processed by a worker |
| `Done`        | `completed`  | Finished successfully |
| `Failed`      | `failed`     | Errored but may have retries remaining |
| `Killed`      | `failed`     | Retries exhausted — permanently failed |

---

### List Tasks

```
GET /tasks?task_type=crawl&status=completed&limit=50&offset=0
```

| Param       | Type   | Default | Description |
|-------------|--------|---------|-------------|
| `task_type` | string | —       | Filter by job type |
| `status`    | string | —       | Filter by status: `pending`, `running`, `completed`, `failed` |
| `limit`     | int    | 50      | Page size (max 200) |
| `offset`    | int    | 0       | Pagination offset |

Response:

```json
{
  "tasks": [ { /* task object */ }, ... ],
  "total": 128
}
```

---

### Get Task

```
GET /tasks/{id}
```

Returns a single task object. Returns `404` if not found.

---

### Delete Task

```
DELETE /tasks/{id}
```

Delete a single task by ID. Also cleans up `task_dependencies` rows. Returns `404` if not found.

Response:

```json
{ "message": "Task deleted" }
```

---

### Delete Pending Tasks

```
DELETE /tasks/pending?task_type=download
```

Convenience endpoint to delete all pending tasks. Equivalent to `POST /tasks/clean` with `flags: ["pending"]`.

| Param       | Type   | Description |
|-------------|--------|-------------|
| `task_type` | string | Optional: only delete pending tasks of this type |

Response:

```json
{ "message": "Pending tasks deleted", "deleted": 15 }
```

---

### Clean Tasks (Bulk Delete)

```
POST /tasks/clean
Content-Type: application/json

{
  "flags": ["completed", "failed"],
  "task_type": "crawl"
}
```

Bulk-delete tasks by status flags. This is the most flexible cleanup endpoint.

#### Request Body

| Field       | Type     | Required | Description |
|-------------|----------|----------|-------------|
| `flags`     | string[] | Yes      | At least one flag. See table below. |
| `task_type` | string   | No       | Only clean tasks of this job type |

#### Flags

| Flag        | Deletes                          | Side Effects |
|-------------|----------------------------------|--------------|
| `completed` | Done tasks                       | None |
| `failed`    | Failed tasks (retries exhausted) | None |
| `cancelled` | Killed tasks (manually terminated) | None |
| `pending`   | Pending tasks                    | None |
| `running`   | Running tasks                    | Aborts all workers, deletes, then re-spawns workers |
| `all`       | All of the above                 | Aborts all workers, deletes, then re-spawns workers |

**Note on `running` / `all`:** When `task_type` is **not** set, these flags abort all Apalis workers before deleting, then re-spawn fresh workers. This causes a brief interruption in job processing. When `task_type` **is** set, workers are NOT aborted — only the matching rows are deleted from the database, and in-flight workers will fail gracefully on their next poll.

#### Response (200)

```json
{ "deleted": 42, "flags": ["completed", "failed"] }
```

#### Errors

- `400` — empty `flags` array or invalid flag value
- `401` — missing or invalid auth token

#### Examples

Clean all completed and failed tasks:

```json
{ "flags": ["completed", "failed"] }
```

Clean everything (nuclear option):

```json
{ "flags": ["all"] }
```

Clean only pending download tasks:

```json
{ "flags": ["pending"], "task_type": "download" }
```

---

### Root Tasks

```
GET /tasks/roots?status=running&limit=20
```

Returns only top-level tasks — jobs that are NOT children in any `task_dependencies` relationship. Use this to see high-level crawl jobs without the noise of their subtasks.

Same query parameters as `GET /tasks`.

Response:

```json
{ "tasks": [...], "total": 5 }
```

---

### Task Tree

```
GET /tasks/{id}/tree
GET /tasks/{id}/tree?flatten=true
```

Returns the full recursive hierarchy of subtasks.

**Default (nested) mode:** Each node contains the job details and a `children` array. Useful for tree UI with expand/collapse.

**Flat mode (`?flatten=true`):** Returns all descendants as a flat `tasks` array. Each item includes `parent_job_id` (direct parent) and `root_job_id` (the root task). Useful for table/list UI with sorting and filtering.

Nested response:

```json
{
  "root_job_id": "01HZ...",
  "children": [
    {
      "job": { "id": "...", "job_type": "download", "status": "completed", ... },
      "children": [
        {
          "job": { "id": "...", "job_type": "color_extract", "status": "pending", ... },
          "children": []
        }
      ]
    }
  ]
}
```

Flat response (`?flatten=true`):

```json
{
  "root_job_id": "01HZ...",
  "tasks": [
    {
      "id": "...",
      "job_type": "download",
      "status": "completed",
      "parent_job_id": "01HZ...",
      "root_job_id": "01HZ...",
      ...
    },
    {
      "id": "...",
      "job_type": "color_extract",
      "status": "pending",
      "parent_job_id": "...",
      "root_job_id": "01HZ...",
      ...
    }
  ]
}
```

---

### Subtasks

```
GET /tasks/{id}/subtasks?task_type=download&status=completed&limit=50
```

Flat (non-recursive) list of direct children. Same query parameters as `GET /tasks`.

Response:

```json
{
  "parent_job_id": "01HZ...",
  "subtasks": [ { /* task object */ }, ... ],
  "total": 12
}
```

---

### Interrupt Subtasks

```
DELETE /tasks/{id}/subtasks?task_type=download
```

Delete all **pending** children of a task. Only deletes children with status `Pending` — running or completed subtasks are left untouched.

| Param       | Type   | Description |
|-------------|--------|-------------|
| `task_type` | string | Optional: only delete children of this type |

Response:

```json
{
  "parent_job_id": "01HZ...",
  "cancelled": 8,
  "child_ids": ["01HZ...", "01HZ...", ...]
}
```

---

## Error Responses

All errors return JSON:

```json
{ "error": "Human-readable error message" }
```

| Status | Meaning |
|--------|---------|
| `400`  | Bad request (invalid parameters or body) |
| `401`  | Unauthorized (missing or invalid JWT) |
| `404`  | Resource not found |
| `500`  | Internal server error (logged, message sanitized) |

---

## Job Types

| Type                    | Description |
|-------------------------|-------------|
| `crawl`                 | Pixiv ranking/user crawl |
| `download`              | Image file download |
| `color_extract`         | KMeans color palette extraction |
| `upload`                | Upload to CDN (DogeCloud OSS) |
| `accessibility_check`   | Alt-text generation |
| `discover`              | Autonomous image discovery |
| `refresh_pixiv_token`   | Pixiv OAuth token refresh |

---

## Task Hierarchy

Tasks form a parent-child tree tracked in the `task_dependencies` table:

```
crawl (root)
├── download #1
│   ├── color_extract
│   ├── upload
│   └── accessibility_check
├── download #2
│   ├── color_extract
│   ├── upload
│   └── accessibility_check
└── ...
```

- Root tasks (crawl, discover) have no parent.
- Download tasks are children of the crawl task that spawned them.
- Pipeline tasks (color_extract, upload, accessibility_check) are children of the root crawl task (via `root_job_id`), not the download task.
- Use `GET /tasks/roots` to see only root tasks.
- Use `GET /tasks/{id}/tree` to see the full hierarchy.
- Use `DELETE /tasks/{id}/subtasks` to cancel all pending children of a task.
