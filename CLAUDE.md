# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Randimg is a Rust backend that crawls images from Pixiv, extracts color palettes via KMeans clustering in CIELAB space, and serves them through an HTTP API. It uses SeaORM with SQLite (dev) / PostgreSQL (prod), Axum for the web layer, and a SQL-backed task queue for background work (crawling, downloading, color extraction).

## Build & Run

```bash
# Build
cargo build

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

Background tasks are stored in the `background_tasks` table. Six tokio workers poll every 5 seconds, each handling one task type: `crawl`, `download`, `color_extract`, `upload`, `accessibility_check`, `discover`. Task claim is atomic (raw SQL `UPDATE ... WHERE id = (SELECT ... LIMIT 1) RETURNING`). Failed tasks retry up to 3 times.

### Color pipeline (`src/color/`)

`extract_theme_colors()` takes an image buffer and returns 10 palette colors (sorted by L*) plus a primary color. Uses KMeans++ with rayon parallelism, mini-batch mode for large inputs, and precomputed sRGB↔linear LUTs. All clustering happens in CIELAB color space.

### Auth (`src/auth/`)

JWT-based with Argon2 password hashing. `AuthUser` and `OptionalAuthUser` are Axum `FromRequestParts` extractors that parse the `Authorization: Bearer` header.

### Configuration (`src/config.rs`)

All settings come from environment variables (loaded via `dotenvy`). The app panics at startup if `SECRET_KEY` is still the default placeholder.

## Key Files

- **`src/main.rs`** — Entry point: config, tracing setup, DB init, task runner startup, Axum router definition, graceful shutdown.
- **`src/lib.rs`** — Crate root: re-exports all modules, defines `AppState` (shared DB connection + config).
- **`src/error.rs`** — `AppError` enum with `IntoResponse`; `From<DbErr>` for automatic conversion.
- **`src/task_queue/mod.rs`** — `submit_task`, `claim_next_task`, `complete_task`, `fail_task`.
- **`src/db/query/image.rs`** — Most complex query file: random selection, paginated list with popularity scoring, color search with bounding-box pre-filter, discover seed selection.
- **`src/color/kmeans.rs`** — KMeans++ with empty-cluster recovery and parallel chunk assignment.
- **`src/handlers/image.rs`** — Image serving with path traversal protection (canonicalize + prefix check).

## Database & Migrations

Migrations live in `migration/` as a path dependency and run automatically on startup. SeaORM dual-database support is gated by feature flags (`sqlite` / `postgres`). New migrations go in `migration/src/` with the `m{YYYYMMDD}_{seq}_{name}.rs` naming convention.

## Environment Variables

See `.env.example` for the full list. Critical ones:
- `DATABASE_URL` — `sqlite://data/randimg.db` (dev) or PostgreSQL connection string
- `SECRET_KEY` — JWT signing secret (must change from default)
- `PIXIV_REFRESH_TOKEN` — Required for Pixiv crawling
- `CDN_BASE_URL` — Prefix for image URLs in API responses
- `IMAGE_DIR` — Local filesystem path for downloaded images

## Conventions

- Error handling: return `AppError` variants, not raw `StatusCode`. Internal errors are logged at ERROR level before returning a sanitized message.
- Batch-fetch associations to avoid N+1 queries (see `list_images` in `db/query/image.rs`).
- New task types: add variant to `TaskType` in `db/entities/task.rs`, add runner branch in `task_queue/runner.rs`, add task logic in `task_queue/tasks/`.
- Routes are registered in `main.rs` inside `build_router()`.
- Use `tracing` macros (`info!`, `error!`, `debug!`) — not `println!`.
