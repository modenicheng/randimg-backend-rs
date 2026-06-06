# Randimg API Reference

Base URL: `http://localhost:8000` (configurable via `SERVER_ADDR` env var)

All timestamps are in UTC+8 (Asia/Shanghai) format: `YYYY-MM-DD HH:MM:SS`.

---

## Authentication

Admin endpoints require a JWT token in the `Authorization` header:

```
Authorization: Bearer <token>
```

### Obtain a Token

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

Response:

```json
{ "status": "ok" }
```

### Random Image

```
GET /
```

Returns a single random image. When color filter params are provided, the image is randomly selected from all matches; when omitted, any accessible image may be returned.

| Param          | Type   | Default | Description |
|----------------|--------|---------|-------------|
| `format`       | string | `json`  | Response format: `json` (object), `image` (302 redirect to CDN) |
| `local`        | bool   | `false` | Serve local file directly instead of CDN URL |
| `ratio_floor`  | float  | `0`     | Minimum aspect ratio (width / height) |
| `ratio_ceil`   | float  | `10`    | Maximum aspect ratio |
| `width_floor`  | int    | `0`     | Minimum width in pixels |
| `width_ceil`   | int    | MAX     | Maximum width in pixels |
| `height_floor` | int    | `0`     | Minimum height in pixels |
| `height_ceil`  | int    | MAX     | Maximum height in pixels |
| `author`       | string | —       | Filter by author name (case-insensitive) |
| `tags`         | string | —       | Comma-separated tag names |
| `rgb`          | string | —       | Target color as `r,g,b` (e.g. `255,0,0`). Mutually exclusive with `lab`. |
| `lab`          | string | —       | Target color as `l,a,b` in CIELAB space (e.g. `50,0,0`). Mutually exclusive with `rgb`. |
| `mode`         | string | `primary` | Color match mode: `primary` (dominant color) or `palette` (color palette) |
| `max_dist`     | float  | `2500`  | Maximum squared LAB distance (ΔE² ≈ 50). Higher = looser match. |

**Color filter details:**

- `rgb` and `lab` are mutually exclusive — providing both is silently ignored.
- `rgb` values are converted to CIELAB internally. Range: 0–255 per channel.
- `lab` values are used directly. L ≈ 0–100, a ≈ -128–127, b ≈ -128–127.
- `mode=primary` compares against the image's dominant color. Faster, fewer results.
- `mode=palette` compares against the image's 10-color palette. Slower, more results.
- `max_dist` is the squared Euclidean distance. Default 2500 ≈ ΔE 50. Lower = more precise match.

### Image Detail

```
GET /image/{id}
```

Returns full metadata for a single image, including author, tags, and color palette.

| Param    | Type   | Default | Description |
|----------|--------|---------|-------------|
| `format` | string | `json`  | Response format: `json` (object), `image` (302 redirect) |
| `local`  | bool   | `false` | Serve local file directly |

### Paginated Image List

```
GET /list
```

Returns a paginated list of images with optional filtering and sorting.

| Param          | Type   | Default | Description |
|----------------|--------|---------|-------------|
| `offset`       | int    | `0`     | Pagination offset (max 100,000) |
| `limit`        | int    | `30`    | Page size (max 300) |
| `desc`         | bool   | `true`  | Sort descending |
| `sort_by`      | string | `id`    | Sort field: `id`, `width`, `height`, `aspect_ratio`, `source_created_at`, `created_at`, `popularity`, `distance` |
| `ratio_floor`  | float  | `0`     | Minimum aspect ratio |
| `ratio_ceil`   | float  | `10`    | Maximum aspect ratio |
| `width_floor`  | int    | `0`     | Minimum width |
| `width_ceil`   | int    | MAX     | Maximum width |
| `height_floor` | int    | `0`     | Minimum height |
| `height_ceil`  | int    | MAX     | Maximum height |
| `author`       | string | —       | Filter by author name |
| `tags`         | string | —       | Comma-separated tag filter |
| `accessible`   | string | —       | **Admin only.** See note below. |
| `rgb`          | string | —       | Target color as `r,g,b`. Mutually exclusive with `lab`. |
| `lab`          | string | —       | Target color as `l,a,b`. Mutually exclusive with `rgb`. |
| `mode`         | string | `primary` | Color match mode: `primary` or `palette` |
| `max_dist`     | float  | `2500`  | Maximum squared LAB distance |

**`sort_by=distance`**: Only meaningful when color filter params are provided. Results are sorted by color distance (closest first). Combined with `max_dist` for filtering.

**`accessible` field semantics:**

- Non-admin users: always filtered to `accessible != false`. The query parameter is ignored.
- Admin users (authenticated): no filter by default. `?accessible=true` shows only accessible images; `?accessible=false` shows only inaccessible images.

### Color Search

```
GET /color/search
```

Dedicated endpoint for color-based image search. Returns results sorted by color distance.

| Param      | Type   | Default | Description |
|------------|--------|---------|-------------|
| `rgb`      | string | —       | Target color as `r,g,b`. Mutually exclusive with `lab`. |
| `lab`      | string | —       | Target color as `l,a,b`. Mutually exclusive with `rgb`. |
| `mode`     | string | `primary` | `primary` or `palette` |
| `max_dist` | float  | —       | Maximum squared LAB distance. No limit if omitted. |
| `limit`    | int    | `20`    | Max results (max 100) |

Returns `400` if neither `rgb` nor `lab` is provided, or if both are provided.

### Statistics

```
GET /statistic
```

Response:

```json
{
  "illust_count": 12345,
  "tag_count": 678,
  "author_count": 234
}
```

### Tags

```
GET /tags?limit=50&offset=0
```

| Param    | Type | Default | Description |
|----------|------|---------|-------------|
| `limit`  | int  | `30`    | Page size (max 300) |
| `offset` | int  | `0`     | Pagination offset |

Response (array):

```json
[
  {
    "id": 1,
    "name": "landscape",
    "translated_name": "风景",
    "search_string": "landscape|风景"
  }
]
```

### Authors

```
GET /authors?limit=30&offset=0
```

| Param    | Type | Default | Description |
|----------|------|---------|-------------|
| `limit`  | int  | `30`    | Page size (max 300) |
| `offset` | int  | `0`     | Pagination offset |

```
GET /authors/{id}
```

Returns a single author with their associated images.

---

## Admin Endpoints (Auth Required)

All endpoints below require `Authorization: Bearer <token>`.

### Image Management

| Method   | Path          | Description |
|----------|---------------|-------------|
| `PATCH`  | `/image/{id}` | Update image metadata |
| `DELETE` | `/image/{id}` | Soft-delete image (marks `deleted_at`, sets `is_public=false`) |

#### PATCH /image/{id}

Request body:

```json
{
  "title": "Sunset over mountains",
  "accessible": true,
  "is_public": true,
  "avatar_available": false,
  "colors": []
}
```

| Field             | Type            | Notes |
|-------------------|-----------------|-------|
| `title`           | string?         | Max 1000 chars, must not be empty |
| `accessible`      | bool \| null    | Controls non-admin visibility |
| `is_public`       | bool?           | Public listing flag |
| `avatar_available`| bool?           | Avatar availability |
| `colors`          | object \| array | Custom color data |

### Tag Management

| Method   | Path        | Description |
|----------|-------------|-------------|
| `PATCH`  | `/tags/{id}` | Update tag translation |
| `DELETE` | `/tags/{id}` | Delete tag (also removes image associations) |

#### PATCH /tags/{id}

```json
{ "translated_name": "风景" }
```

### Crawler

| Method | Path                 | Description |
|--------|----------------------|-------------|
| `GET`  | `/crawler`           | List crawler tasks |
| `POST` | `/crawler`           | Create a crawl job |
| `GET`  | `/crawler/{id}`      | Get crawler task detail |
| `GET`  | `/crawler/image`     | Get crawl-related image info |
| `POST` | `/crawler/discover`  | Trigger autonomous image discovery |
| `GET`  | `/admin/accessibility-queue` | List accessibility check queue |

#### POST /crawler

```json
{
  "task_name": "Daily ranking crawl",
  "crawl_type": 0,
  "target_start_date": "2026-06-01T00:00:00",
  "target_end_date": "2026-06-07T00:00:00",
  "ranking_mode": "day",
  "illust_type_filter": ["illust"],
  "exclude_r18": true,
  "exclude_ai": false,
  "max_pages": 10,
  "discover_hops": 3,
  "discover_seed_limit": 5,
  "discover_seed_method": "popularity",
  "disable_discover": false,
  "credential_ids": [1]
}
```

| Field                | Type       | Required | Description |
|----------------------|------------|----------|-------------|
| `task_name`          | string     | No       | Human-readable label |
| `crawl_type`         | int        | No       | `0` = ranking, `1` = user, `2` = bookmarks (default: `1`) |
| `target_user_id`     | string     | `crawl_type=1` | Pixiv user ID |
| `target_start_date`  | datetime   | `crawl_type=0` | Ranking start date |
| `target_end_date`    | datetime   | `crawl_type=0` | Ranking end date |
| `target_search_prompt`| string    | No       | Search prompt for keyword-based crawl |
| `ranking_mode`       | string     | No       | `day`, `week`, `month`, `original`, `rookie` (default: `day`) |
| `illust_type_filter` | string[]   | No       | Filter by illust type: `["illust"]`, `["manga"]`, etc. |
| `exclude_r18`        | bool       | No       | Exclude R18 content (default: false) |
| `exclude_ai`         | bool       | No       | Exclude AI-generated content (default: false) |
| `max_pages`          | int        | No       | Max pages to crawl (0 = unlimited) |
| `discover_hops`      | int        | No       | Discovery hops after crawl (default: global) |
| `discover_seed_limit`| int        | No       | Seed images for discovery (default: global) |
| `discover_seed_method`| string    | No       | `popularity`, `views`, `bookmarks`, `random` |
| `disable_discover`   | bool       | No       | Skip discovery after crawl (default: false) |
| `credential_ids`     | int[]      | `crawl_type=2` | Pixiv credential IDs to use |

### Pixiv Credentials

| Method  | Path                            | Description |
|---------|---------------------------------|-------------|
| `GET`   | `/pixiv-credential`             | List all credentials |
| `POST`  | `/pixiv-credential`             | Add a new credential |
| `GET`   | `/pixiv-credential/{id}`        | Get credential detail |
| `PATCH` | `/pixiv-credential/{id}`        | Update credential |
| `DELETE`| `/pixiv-credential/{id}`        | Delete credential |
| `POST`  | `/pixiv-credential/{id}/refresh`| Submit token refresh task |
| `GET`   | `/pixiv-credential/{id}/token`  | Get current access token |

#### POST /pixiv-credential

```json
{
  "pixiv_user_id": "12345678",
  "refresh_token": "abc...",
  "note": "Main account"
}
```

---

## Task Management Endpoints

All task endpoints require authentication.

### Common Response Fields

| Field          | Type    | Description |
|----------------|---------|-------------|
| `id`           | string  | ULID task identifier |
| `job_type`     | string  | Job type (see [Job Types](#job-types)) |
| `status`       | string  | `pending`, `queued`, `running`, `completed`, `failed`, `killed` |
| `attempts`     | int     | Attempt count |
| `max_attempts` | int     | Max attempts before permanent failure |
| `run_at`       | string  | Scheduled run time (UTC+8) |
| `done_at`      | string? | Completion time (null if not done) |
| `last_result`  | string? | Error message from last attempt |
| `payload`      | object? | Deserialized job parameters |

### List Tasks

```
GET /tasks?task_type=crawl&status=completed&limit=50&offset=0
```

| Param       | Type   | Default | Description |
|-------------|--------|---------|-------------|
| `task_type` | string | —       | Filter by job type |
| `status`    | string | —       | Filter by status: `pending`, `running`, `completed`, `failed`, `killed` |
| `limit`     | int    | `50`    | Page size (max 200) |
| `offset`    | int    | `0`     | Pagination offset |

Response:

```json
{ "tasks": [...], "total": 128 }
```

### Get Task

```
GET /tasks/{id}
```

Returns a single task object. `404` if not found.

### Delete Task

```
DELETE /tasks/{id}
```

```json
{ "message": "Task deleted" }
```

### Delete Pending Tasks

```
DELETE /tasks/pending?task_type=download
```

| Param       | Type   | Description |
|-------------|--------|-------------|
| `task_type` | string | Optional: only delete pending tasks of this type |

Response:

```json
{ "message": "Pending tasks deleted", "deleted": 15 }
```

### Clean Tasks (Bulk Delete)

```
POST /tasks/clean
Content-Type: application/json

{ "flags": ["completed", "failed"], "task_type": "crawl" }
```

| Field       | Type     | Required | Description |
|-------------|----------|----------|-------------|
| `flags`     | string[] | Yes      | See flag table below |
| `task_type` | string   | No       | Only clean tasks of this job type |
| `crawl_type`| int      | No       | Filter crawl tasks: `0`=ranking, `1`=user, `2`=bookmarks |

**Flags:**

| Flag        | Deletes |
|-------------|---------|
| `completed` | Done tasks |
| `failed`    | Failed tasks (retries exhausted) |
| `cancelled` | Manually terminated tasks |
| `killed`    | Killed tasks |
| `pending`   | Pending tasks |
| `running`   | Running tasks |
| `all`       | All of the above |

Response:

```json
{ "deleted": 42, "flags": ["completed", "failed"] }
```

### Root Tasks

```
GET /tasks/roots?status=running&limit=20
```

Returns only top-level tasks (no parent). Same query params as `GET /tasks`.

### Task Tree

```
GET /tasks/{id}/tree
GET /tasks/{id}/tree?flatten=true
```

**Nested mode (default):** Each node has a `children` array for recursive hierarchy.

```json
{
  "root_job_id": "01HZ...",
  "children": [
    {
      "job": { "id": "...", "job_type": "download", "status": "completed" },
      "children": [
        {
          "job": { "id": "...", "job_type": "color_extract", "status": "pending" },
          "children": []
        }
      ]
    }
  ]
}
```

**Flat mode (`?flatten=true`):** All descendants as a flat `tasks` array with `parent_job_id` and `root_job_id` references.

### Subtasks

```
GET /tasks/{id}/subtasks?task_type=download&status=completed&limit=50
```

Flat list of direct children. Same query params as `GET /tasks`.

```json
{ "parent_job_id": "01HZ...", "subtasks": [...], "total": 12 }
```

### Interrupt Subtasks

```
DELETE /tasks/{id}/subtasks?task_type=download
```

Delete all **pending** children of a task. Running/completed subtasks are left untouched.

| Param       | Type   | Description |
|-------------|--------|-------------|
| `task_type` | string | Optional: only delete children of this type |

Response:

```json
{
  "parent_job_id": "01HZ...",
  "cancelled": 8,
  "child_ids": ["01HZ...", "01HZ..."]
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
| `400`  | Bad request (invalid parameters, body, or combination like `rgb`+`lab`) |
| `401`  | Unauthorized (missing or invalid JWT) |
| `404`  | Resource not found |
| `500`  | Internal server error (logged, message sanitized) |

---

## Job Types

| Type                    | Description |
|-------------------------|-------------|
| `crawl`                 | Pixiv ranking/user/bookmarks crawl |
| `download`              | Image file download |
| `color_extract`         | KMeans color palette extraction |
| `upload`                | Upload to CDN (DogeCloud OSS) |
| `accessibility_check`   | Alt-text generation / accessibility check |
| `discover`              | Autonomous image discovery |
| `refresh_pixiv_token`   | Pixiv OAuth token refresh |
| `cleanup`               | Task queue maintenance |

---

## Task Hierarchy

Tasks form a parent-child tree via `parent_id`/`root_id` columns:

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
- Download tasks are children of the crawl task.
- Pipeline tasks (color_extract, upload, accessibility_check) are children of the root task.
- `GET /tasks/roots` — see only root tasks.
- `GET /tasks/{id}/tree` — full hierarchy.
- `DELETE /tasks/{id}/subtasks` — cancel pending children.
