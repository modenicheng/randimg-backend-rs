# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Knowledge Base**: See [AGENTS.md](AGENTS.md) for the hierarchical project knowledge base with confidence ratings and source tracking.

## Project Overview

Randimg is a Rust backend that crawls images from Pixiv, extracts color palettes via KMeans clustering in CIELAB space, and serves them through an HTTP API. It uses SeaORM with PostgreSQL, Axum for the web layer, and Fang for background job processing (crawling, downloading, color extraction).

## Build & Run

```bash
# Build (PostgreSQL API + PostgreSQL queue)
cargo build

# Run server
cargo run -p randimg-server

# Run worker (headless, no Axum)
cargo run -p randimg-worker

# Run tests (skip color tests — they crash machines)
cargo test -p randimg-core -- --skip kmeans --skip extract_theme --skip histogram --skip palette --skip primary_color --skip lab_round

# Create admin user
cargo run -p randimg-server --bin create-admin

# Color extraction demo
cargo run -p randimg-core --example color_extract_demo -- path/to/image.jpg
```

## Architecture

### Workspace structure

```
Cargo.toml                    # Virtual workspace root
├── crates/randimg-core/      # Library: WorkerState, all shared code
├── crates/randimg-server/    # Binary: ServerState wraps WorkerState, Axum HTTP
├── crates/randimg-worker/    # Binary: WorkerState only, headless workers (NO axum)
└── migration/                # SeaORM migrations
```

### State types

- **`WorkerState`** (core crate): db, config, oss, http_client, queue
- **`ServerState`** (server crate): wraps WorkerState

Core task handlers access global state via `OnceCell<Arc<WorkerState>>`. Server Axum handlers use `State<Arc<WorkerState>>` (passed via `.with_state(Arc::new(state.worker.clone()))`).

Workers run in the separate `randimg-worker` binary — the server only serves HTTP. The server pushes jobs to the queue (e.g. refresh-pixiv-token on startup) but does not run workers.

- **`crates/randimg-core/src/handlers/`** — Axum route handlers. Each handler receives `State<Arc<WorkerState>>` and optionally `AuthUser`/`OptionalAuthUser` extractors.
- **`crates/randimg-core/src/db/query/`** — Database query functions, one file per entity. This is where SeaORM `Select`, `Insert`, `Update`, `Delete` operations live.
- **`crates/randimg-core/src/db/entities/`** — SeaORM entity models with `DeriveEntityModel` and relation definitions.

### Task queue (`crates/randimg-core/src/task_queue/`)

Uses the Fang library with async PostgreSQL backend. Database separation:

- **`API_DATABASE_URL`** — SeaORM manages business data + `tasks` table for task metadata
- **`QUEUE_DATABASE_URL`** — Fang manages task scheduling (PostgreSQL)

Seven workers, one per job type: `crawl`, `download`, `color_extract`, `upload`, `accessibility_check`, `discover`, `refresh_pixiv_token`. Jobs implement the `AsyncRunnable` trait with `#[typetag::serde]` + `#[async_trait]`. Workers are spawned via the `spawn_workers()` function in `src/lib.rs` (uses a local `spawn_worker!` macro). The backend abstraction lives in `src/db_backend.rs`.

**Task push flow**: `query::task::create()` creates a row in the `tasks` table → `queue.insert_task()` enqueues it in Fang → `query::task::link_fang_task()` links the `fang_task_id` back to the tasks table.

The worker binary (`crates/randimg-worker/`) runs all 7 workers without Axum — true headless operation for distributed deployment.

**Retry policy design**: Configured via `TASK_MAX_RETRIES` (default: 3) and `TASK_BACKOFF_BASE` (default: 2) for exponential backoff. Network-bound tasks use retries; compute-bound tasks (color_extract) can be configured with fewer retries since KMeans is deterministic.

| Task Type | Retry Policy | Rationale |
|-----------|--------------|-----------|
| crawl, download, upload, accessibility-check, discover, refresh-pixiv-token | Exponential backoff, max retries | Network-bound (API calls, HTTP requests) |
| color_extract | Minimal retries | Compute-bound (KMeans is deterministic) |

**Job pipeline**: Crawl → Download → (ColorExtract + Upload + AccessibilityCheck in parallel) → Discover. Parent-child relationships are tracked in `task_dependencies` table. `DownloadJob` has a `root_job_id` so downstream tasks appear as direct children of the crawl task. The `db_backend.rs` provides methods that pre-generate ULIDs to record hierarchy before jobs execute.

#### Color worker process isolation

The color extraction worker (KMeans + rayon) is CPU-intensive. Two modes:

1. **In-process** (default): color-extract runs as a Fang worker inside the main binary. `spawn_blocking` + dedicated rayon pool (`crates/randimg-core/src/color/mod.rs::run_on_color_pool`) prevent it from blocking the async runtime.

2. **Separate process** (`COLOR_WORKER_STANDALONE=true`): the server binary skips spawning the color-extract worker. Run `cargo run -p randimg-worker` as a separate process — it runs all 7 workers including color-extract. Both binaries connect to the same database; Fang storage handles coordination.

Environment variables:
- `COLOR_WORKER_STANDALONE` — `true` to exclude color-extract from server binary
- `COLOR_WORKER_RAYON_THREADS` — rayon thread count for color extraction (default: CPU count)

#### Fang status semantics

Fang uses different status names than the previous Apalis backend. The `tasks` table in the API database maintains its own status set, synced with Fang via `fang_task_id`:

| Condition | Task status |
|-----------|-------------|
| Task created, not yet picked up | `pending` |
| Worker is executing | `running` |
| Handler returns `Ok` | `completed` |
| Handler returns `Err`, retries remain | `failed` (transient) |
| Handler returns `Err`, retries exhausted | `failed` (terminal) |
| Manually cancelled | `cancelled` |

The API exposes these statuses directly. The `failed` state covers both transient failures (will be retried) and terminal failures (retries exhausted). The `killed` status from the previous Apalis backend has been removed.

#### Derived status for root tasks (`GET /tasks/roots`)

`list_roots_derived` (PostgreSQL recursive CTE in `src/db/query/task_tree.rs`) computes four boolean flags per root from its descendant subtree:

| Flag | Meaning |
|---|---|
| `has_active` | At least one descendant is `Pending` / `Running`. |
| `has_failed` | At least one descendant is `Failed`. |
| `has_completed` | At least one descendant is `Completed`. |

These feed two layers of logic:

1. **Dead-subtree short-circuit** in `list_roots::effective` (`src/handlers/task.rs`):
   - If `has_active == false && has_failed && !has_completed` — every failed descendant is terminal, no path to recovery — the API `status` is forced to `"failed"` even if the root itself is still retrying.
2. **Otherwise root-priority + rollup**:
   - If the root's own status is anything other than `Completed` → use `map_status(root.status)` directly.
   - If the root is `Completed` and has descendants → call `derived_status_from_flags(active, failed, completed)`:
     - `has_active` → `"running"` (any in-flight descendant overrides everything).
     - `has_failed && has_completed` → `"partial_success"`.
     - `has_failed` only → `"failed"`.
     - `has_completed` only → `"completed"`.
     - else → `"pending"`.
   - If the root is `Completed` and has no descendants → `"completed"`.

The same logic is also used by the `derived_status` filter in `list_roots_derived` and `count_roots_derived` so that filters match the rollup output exactly (priority: `failed > partial_success > running > completed > pending`).

#### Task concurrency

Each worker's concurrency is configurable via environment variables:

| Worker | Env Var | Default |
|--------|---------|---------|
| crawl | `TASK_CONCURRENCY_CRAWL` | 2 |
| download | `TASK_CONCURRENCY_DOWNLOAD` | 4 |
| color_extract | `TASK_CONCURRENCY_COLOR_EXTRACT` | 2 |
| upload | `TASK_CONCURRENCY_UPLOAD` | 2 |
| accessibility_check | `TASK_CONCURRENCY_ACCESSIBILITY_CHECK` | 2 |
| discover | `TASK_CONCURRENCY_DISCOVER` | 1 |
| refresh_pixiv_token | `TASK_CONCURRENCY_REFRESH_PIXIV_TOKEN` | 1 |

### Color pipeline (`src/color/`)

`extract_theme_colors()` takes an image buffer and returns 10 palette colors (sorted by L*) plus a primary color. Uses KMeans++ with rayon parallelism, mini-batch mode for large inputs, and precomputed sRGB↔linear LUTs. All clustering happens in CIELAB color space.

### Auth (`src/auth/`)

JWT-based with Argon2 password hashing. `AuthUser` and `OptionalAuthUser` are Axum `FromRequestParts` extractors that parse the `Authorization: Bearer` header.

### Configuration (`src/config.rs`)

All settings come from environment variables (loaded via `dotenvy`). The app panics at startup if `SECRET_KEY` is still the default placeholder.

## Key Files

- **`crates/randimg-server/src/main.rs`** — Server entry point: config, tracing, DB init, Axum router, graceful shutdown.
- **`crates/randimg-worker/src/main.rs`** — Worker entry point: config, tracing, DB init, Fang workers, graceful shutdown. NO Axum.
- **`crates/randimg-core/src/lib.rs`** — Crate root: re-exports all modules, defines `WorkerState` (shared DB connection + config).
- **`crates/randimg-core/src/config.rs`** — `AppConfig` struct with all env vars including `api_database_url` and `queue_database_url`.
- **`crates/randimg-core/src/db_backend.rs`** — Feature-gated queue backend init, `AsyncRunnable` task definitions.
- **`crates/randimg-core/src/error.rs`** — `AppError` enum with `IntoResponse`; `From<DbErr>` for automatic conversion.
- **`crates/randimg-core/src/task_queue/jobs.rs`** — Job struct definitions (one per task type), implementing `AsyncRunnable`.
- **`crates/randimg-core/src/task_queue/handlers.rs`** — Job handler functions (one per task type).
- **`crates/randimg-core/src/db/query/image.rs`** — Most complex query file: random selection, paginated list with popularity scoring, color search with bounding-box pre-filter, discover seed selection.
- **`crates/randimg-core/src/db/query/task_tree.rs`** — Recursive CTE (PostgreSQL) computing `has_active` / `has_failed` / `has_completed` flags per root; `derived_status_from_flags` implements the rollup; `list_roots_derived` / `count_roots_derived` apply derived-status filters in SQL.
- **`crates/randimg-core/src/color/kmeans.rs`** — KMeans++ with empty-cluster recovery and parallel chunk assignment.
- **`crates/randimg-core/src/handlers/image.rs`** — Image serving with path traversal protection (canonicalize + prefix check).

## Database & Migrations

Migrations live in `migration/` as a path dependency and run automatically on startup. The API database and queue database are separate: `API_DATABASE_URL` for SeaORM business data, `QUEUE_DATABASE_URL` for Fang task scheduling. New migrations go in `migration/src/` with the `m{YYYYMMDD}_{seq}_{name}.rs` naming convention.

## Environment Variables

See `.env.example` for the full list. Critical ones:
- `API_DATABASE_URL` — PostgreSQL connection string for API database
- `QUEUE_DATABASE_URL` — PostgreSQL connection string for Fang queue database
- `SECRET_KEY` — JWT signing secret (must change from default)
- `PIXIV_REFRESH_TOKEN` — Required for Pixiv crawling
- `CDN_BASE_URL` — Prefix for image URLs in API responses
- `IMAGE_DIR` — Local filesystem path for downloaded images
- `SERVER_ADDR` — Supports TCP (`0.0.0.0:8000`) and Unix socket (`unix:///run/randimg.sock`)
- `TASK_MAX_RETRIES` — Max retry count for failed tasks (default: 3)
- `TASK_BACKOFF_BASE` — Exponential backoff base in seconds (default: 2)

## Conventions

- Error handling: return `AppError` variants, not raw `StatusCode`. Internal errors are logged at ERROR level before returning a sanitized message.
- Each handler module exports a `routes()` function returning an `Axum Router`. Routes are merged in `crates/randimg-server/src/main.rs`.
- Batch-fetch associations to avoid N+1 queries (see `list_images` in `db/query/image.rs`).
- New task types: define a job struct implementing `AsyncRunnable` in `task_queue/jobs.rs`, add a handler in `task_queue/handlers.rs`, register a worker in `lib.rs::spawn_workers()`.
- Routes are registered in `crates/randimg-server/src/main.rs` by adding a `.merge(module::routes())` call to the router.
- Use `tracing` macros (`info!`, `error!`, `debug!`) — not `println!`.
- Task status exposure: `map_status()` maps internal statuses to API values. The root rollup in `list_roots::effective` aggregates them via the `has_failed` / `has_completed` flags — see the "Derived status for root tasks" section above. When adding new derived-status logic, update `derived_status_from_flags` in `src/db/query/task_tree.rs`, the `derived_status` filter in `list_roots_derived` / `count_roots_derived` (priority: `failed > partial_success > running > completed > pending`), and any new unit tests in `tests/db_test.rs`.
