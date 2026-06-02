# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Randimg is a Rust backend that crawls images from Pixiv, extracts color palettes via KMeans clustering in CIELAB space, and serves them through an HTTP API. It uses SeaORM with SQLite (dev) / PostgreSQL (prod), Axum for the web layer, and Apalis for background job processing (crawling, downloading, color extraction).

## Build & Run

```bash
# Build (default SQLite)
cargo build

# Build with PostgreSQL
cargo build --no-default-features --features postgres

# Run (requires .env with SECRET_KEY set to non-default value)
cargo run

# Run tests (color unit tests run standalone; API tests require a running server)
cargo test

# Run a single test
cargo test test_name_here

# Create an admin user (interactive CLI)
cargo run --bin create_admin

# Run the color extraction benchmark demo
cargo run --example color_extract_demo -- path/to/image.jpg
```

## Architecture

### Layered structure: handlers → db/query → entities

- **`src/handlers/`** — Axum route handlers. Each handler receives `State<Arc<AppState>>` and optionally `AuthUser`/`OptionalAuthUser` extractors.
- **`src/db/query/`** — Database query functions, one file per entity. This is where SeaORM `Select`, `Insert`, `Update`, `Delete` operations live.
- **`src/db/entities/`** — SeaORM entity models with `DeriveEntityModel` and relation definitions.

### Task queue (`src/task_queue/`)

Uses the Apalis library with `apalis-sqlite` (dev) / `apalis-postgres` (prod) for job storage. Seven workers, one per job type: `crawl`, `download`, `color_extract`, `upload`, `accessibility_check`, `discover`, `refresh_pixiv_token`. Jobs are defined as structs in `src/task_queue/jobs.rs`, handlers live in `src/task_queue/handlers.rs`, and workers are spawned via the `spawn_workers()` function in `src/lib.rs` (uses a local `spawn_worker!` macro). Failed tasks retry up to 3 times (Apalis built-in). The backend abstraction (`Pool`, `JobStorage`, `init()`) lives in `src/db_backend.rs`.

**Job pipeline**: Crawl → Download → (ColorExtract + Upload + AccessibilityCheck in parallel) → Discover. Parent-child relationships are tracked in `task_dependencies` table. `DownloadJob` has a `root_job_id` so downstream tasks appear as direct children of the crawl task. `db_backend.rs` provides `push_*_with_parent()` methods that pre-generate ULIDs to record hierarchy before jobs execute.

#### Color worker process isolation

The color extraction worker (KMeans + rayon) is CPU-intensive. Two modes:

1. **In-process** (default): color-extract runs as an Apalis worker inside the main binary. `spawn_blocking` + dedicated rayon pool (`src/color/mod.rs::run_on_color_pool`) prevent it from blocking the async runtime.

2. **Separate process** (`COLOR_WORKER_STANDALONE=true`): the main binary skips spawning the color-extract worker. Run `cargo run --bin color-worker` as a separate process. Both binaries connect to the same database; Apalis storage handles coordination.

Environment variables:
- `COLOR_WORKER_STANDALONE` — `true` to exclude color-extract from main binary
- `COLOR_WORKER_RAYON_THREADS` — rayon thread count for color extraction (default: CPU count)
- `COLOR_WORKER_AUTO_SPAWN` — `true` to auto-spawn color-worker as child process from main

#### Apalis status semantics — "Killed" means "retries exhausted", NOT "manually cancelled"

This is a common source of confusion. Apalis's `calculate_status()` (in both `apalis-sqlite` and `apalis-postgres`) uses these rules:

| Condition | DB status |
|-----------|-----------|
| Handler returns `Ok` | `"Done"` |
| Handler returns `Err`, retries remain | `"Failed"` (transient) |
| Handler returns `Err`, retries exhausted | **`"Killed"`** (terminal) |
| Handler returns `AbortError` | **`"Killed"`** |

In Apalis, **`"Failed"` is a transient state** (the attempt failed but will be retried), and **`"Killed"` is the terminal failure state** (all retries exhausted, job is dead). This means a task that fails after exhausting all retries gets status `"Killed"` in the database — it was never manually cancelled.

The frontend already has a dedicated chip for `"killed"` (red, label "失败"), so we deliberately keep `STATUS_FAILED → "failed"` and `STATUS_KILLED → "killed"` as **distinct** API values rather than collapsing them — see the derived-status rules below for how the rollup treats them.

#### Derived status for root tasks (`GET /tasks/roots`)

`list_roots_derived` (SQLite/PostgreSQL recursive CTE in `src/db/query/task_tree.rs`) computes four boolean flags per root from its descendant subtree:

| Flag | Meaning |
|---|---|
| `has_active` | At least one descendant is `Pending` / `Queued` / `Running`. |
| `has_failed` | At least one descendant is `Failed` **or** `Killed`. |
| `has_completed` | At least one descendant is `Done`. |
| `has_killed_terminal` | At least one descendant is in the terminal `Killed` state specifically (subset of `has_failed`). |

These feed two layers of logic:

1. **Dead-subtree short-circuit** in `list_roots::effective` (`src/handlers/task.rs`):
   - If `has_active == false && has_failed && !has_completed && has_killed_terminal == has_failed` — every failed descendant is terminal, no path to recovery — the API `status` is forced to `"killed"` even if the root itself is still retrying. This is the invariant "if all descendants are definitively done and none succeeded, the root is dead too".
2. **Otherwise root-priority + rollup**:
   - If the root's own Apalis status is anything other than `Done` → use `map_status(root.status)` directly.
   - If the root is `Done` and has descendants → call `derived_status_from_flags(active, failed, completed, killed)`:
     - `has_active` → `"running"` (any in-flight descendant overrides everything).
     - `has_failed && has_completed` → `"partial_success"`.
     - `has_failed` only (with `has_killed_terminal < has_failed`) → `"failed"` (some descendants still transient; retries may save them).
     - `has_completed` only → `"completed"`.
     - else → `"pending"` (rollup not normally consulted here, but degrades safely).
   - If the root is `Done` and has no descendants → `"completed"`.

The same `has_killed_terminal` flag is also used by the `derived_status` filter in `list_roots_derived` and `count_roots_derived` so that the "killed" filter matches the rollup output exactly (priority: `killed > failed > partial_success > running > completed > pending`).

#### ⚠️ RetryPolicy vs max_attempts mismatch

`spawn_workers()` in `src/lib.rs` configures `RetryPolicy::retries(3)` (in-memory Tower retry middleware), but does **not** set `max_attempts` on the SQL context. The default `max_attempts` in Apalis SQL is **5** (see `apalis-sql/src/context.rs`). These are two independent retry mechanisms:

- `RetryPolicy::retries(3)` — in-memory retries that re-call the service without touching the DB.
- `max_attempts` — the SQL backend's ack logic decides `Failed` vs `Killed` based on this value.

If you want 3 total attempts, set both `RetryPolicy::retries(2)` and `.max_attempts(3)`. Otherwise the job may be retried more times than expected before reaching terminal `"Killed"` status.

### Color pipeline (`src/color/`)

`extract_theme_colors()` takes an image buffer and returns 10 palette colors (sorted by L*) plus a primary color. Uses KMeans++ with rayon parallelism, mini-batch mode for large inputs, and precomputed sRGB↔linear LUTs. All clustering happens in CIELAB color space.

### Auth (`src/auth/`)

JWT-based with Argon2 password hashing. `AuthUser` and `OptionalAuthUser` are Axum `FromRequestParts` extractors that parse the `Authorization: Bearer` header.

### Configuration (`src/config.rs`)

All settings come from environment variables (loaded via `dotenvy`). The app panics at startup if `SECRET_KEY` is still the default placeholder.

## Key Files

- **`src/main.rs`** — Entry point: config, tracing setup, DB init, task runner startup, Axum router definition (inline via `.merge()` on each handler's `routes()` function), graceful shutdown (SIGINT/SIGTERM).
- **`src/lib.rs`** — Crate root: re-exports all modules, defines `AppState` (shared DB connection + config).
- **`src/error.rs`** — `AppError` enum with `IntoResponse`; `From<DbErr>` for automatic conversion.
- **`src/db_backend.rs`** — Database backend abstraction: `Pool` type, `JobStorage` (holds typed job storages), `init()` to connect and set up Apalis.
- **`src/task_queue/jobs.rs`** — Job struct definitions (one per task type).
- **`src/task_queue/handlers.rs`** — Job handler functions (one per task type).
- **`src/task_queue/mod.rs`** — Re-exports jobs and handlers.
- **`src/db/query/image.rs`** — Most complex query file: random selection, paginated list with popularity scoring, color search with bounding-box pre-filter, discover seed selection.
- **`src/db/query/task_tree.rs`** — Recursive CTE (SQLite + PostgreSQL) computing `has_active` / `has_failed` / `has_completed` / `has_killed_terminal` flags per root; `derived_status_from_flags` implements the rollup; `list_roots_derived` / `count_roots_derived` apply derived-status filters in SQL.
- **`src/color/kmeans.rs`** — KMeans++ with empty-cluster recovery and parallel chunk assignment.
- **`src/handlers/image.rs`** — Image serving with path traversal protection (canonicalize + prefix check).

## Database & Migrations

Migrations live in `migration/` as a path dependency and run automatically on startup. SeaORM dual-database support is gated by feature flags (`sqlite` / `postgres`) — these are mutually exclusive. The `apalis_job` entity is feature-gated for type differences between SQLite (`i64` timestamps, `String` metadata) and PostgreSQL (`DateTimeWithTimeZone`, `JsonValue`). New migrations go in `migration/src/` with the `m{YYYYMMDD}_{seq}_{name}.rs` naming convention.

## Environment Variables

See `.env.example` for the full list. Critical ones:
- `DATABASE_URL` — `sqlite://data/randimg.db` (dev) or PostgreSQL connection string
- `SECRET_KEY` — JWT signing secret (must change from default)
- `PIXIV_REFRESH_TOKEN` — Required for Pixiv crawling
- `CDN_BASE_URL` — Prefix for image URLs in API responses
- `IMAGE_DIR` — Local filesystem path for downloaded images
- `SERVER_ADDR` — Supports TCP (`0.0.0.0:8000`) and Unix socket (`unix:///run/randimg.sock`)

## Conventions

- Error handling: return `AppError` variants, not raw `StatusCode`. Internal errors are logged at ERROR level before returning a sanitized message.
- Each handler module exports a `routes()` function returning an `Axum Router`. Routes are merged in `main.rs`.
- Batch-fetch associations to avoid N+1 queries (see `list_images` in `db/query/image.rs`).
- New task types: define a job struct in `task_queue/jobs.rs`, add a handler in `task_queue/handlers.rs`, add a storage field in `db_backend.rs::JobStorage`, register a worker in `lib.rs::spawn_workers()`.
- Routes are registered in `main.rs` by adding a `.merge(module::routes())` call to the router.
- Use `tracing` macros (`info!`, `error!`, `debug!`) — not `println!`.
- Task status exposure: `map_status()` keeps Apalis `Failed` and `Killed` as **distinct** API values (`failed` and `killed`). The root rollup in `list_roots::effective` aggregates them via the `has_failed` / `has_killed_terminal` flags — see the "Derived status for root tasks" section above. When adding new derived-status logic, update `derived_status_from_flags` in `src/db/query/task_tree.rs`, the `derived_status` filter in `list_roots_derived` / `count_roots_derived` (priority: `killed > failed > partial_success > running > completed > pending`), and any new unit tests in `tests/db_test.rs`.
