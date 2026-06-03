# AGENTS.md — Project Knowledge Base

This file serves as the hierarchical knowledge base for the Randimg Backend project.

## Code Structure [CONFIDENCE: high]

<!-- Source: README.md, workspace structure -->

```
Cargo.toml                    # Virtual workspace root
├── crates/randimg-core/      # Library: WorkerState, all shared code
│   ├── src/
│   │   ├── lib.rs            # Crate root: WorkerState definition, module re-exports
│   │   ├── config.rs         # AppConfig from environment variables (incl. api/queue db URLs)
│   │   ├── error.rs          # AppError enum with IntoResponse
│   │   ├── db_backend.rs     # Queue backend abstraction (Pool / JobStorage / init)
│   │   ├── auth/             # JWT + Argon2 authentication
│   │   ├── color/            # KMeans++ color extraction (CIELAB space)
│   │   ├── db/
│   │   │   ├── entities/     # SeaORM entity models
│   │   │   └── query/        # Database query functions
│   │   ├── dogecloud/        # DogeCloud OSS integration
│   │   ├── handlers/         # Axum route handlers
│   │   ├── pixiv/            # Pixiv API client wrapper
│   │   └── task_queue/       # Job definitions + worker handlers
│   └── tests/
├── crates/randimg-server/    # Binary: Axum HTTP server
│   └── src/main.rs           # ServerState, routes, graceful shutdown
├── crates/randimg-worker/    # Binary: headless workers (NO axum)
│   └── src/main.rs           # WorkerState, Monitor with all 7 workers
└── migration/
    └── src/                  # SeaORM migrations (auto-executed)
```

**Layered architecture**: `handlers → db/query → db/entities`

**Feature flags**:
- `db-sqlite` (default) / `db-postgres` — SeaORM database backend (mutually exclusive)
- `queue-postgres` (default) / `queue-sqlite` — Fang queue backend (mutually exclusive)

## Coding Style [CONFIDENCE: high]

<!-- Source: CLAUDE.md, code inspection -->

- **Rust Edition 2024** — Use modern Rust features
- **Error handling**: Return `AppError` variants, not raw `StatusCode`. Internal errors logged at ERROR level before returning sanitized message
- **Logging**: Use `tracing` macros (`info!`, `error!`, `debug!`) — not `println!`
- **Module organization**: Each handler module exports a `routes()` function returning an Axum Router
- **Route registration**: Add `.merge(module::routes())` call in `main.rs`
- **Database queries**: Batch-fetch associations to avoid N+1 queries
- **Serialization**: Use `serde` with `Serialize`/`Deserialize` derives
- **Configuration**: All settings from environment variables via `dotenvy`

## Testing [CONFIDENCE: high]

<!-- Source: README.md, tests/ directory -->

**Running tests**:
```bash
# Run all tests
cargo test

# Run single test
cargo test test_name_here
```

**Test organization**:
- Unit tests in `tests/` directory (one file per module)
- Color unit tests run standalone
- API tests require a running server
- Test assets in `tests/assets/`

**Test patterns**:
- Use `#[tokio::test]` for async tests
- Use `serde_json` for JSON serialization tests
- Use `sea-orm` mock for database tests

## Error Handling [CONFIDENCE: high]

<!-- Source: CLAUDE.md, src/error.rs -->

- **Custom error type**: `AppError` enum with `IntoResponse` implementation
- **Database errors**: Automatic conversion from `DbErr` via `From<DbErr>`
- **Logging**: Internal errors logged at ERROR level before returning sanitized message
- **HTTP responses**: Return appropriate status codes (400, 401, 404, 500)

## Project Layout [CONFIDENCE: high]

<!-- Source: README.md, Cargo.toml -->

**Top-level files**:
- `Cargo.toml` — Rust package manifest with dependencies
- `Cargo.lock` — Dependency lock file
- `.env` / `.env.example` — Environment variables
- `README.md` — Project documentation
- `CLAUDE.md` — Claude Code guidance
- `LICENSE` — MIT License

**Key directories**:
- `src/` — Main application code
- `migration/` — Database migrations
- `tests/` — Test files
- `docs/` — Documentation
- `examples/` — Example code
- `data/` — SQLite database storage
- `images/` — Downloaded images
- `logs/` — Application logs

## Dependencies [CONFIDENCE: high]

<!-- Source: Cargo.toml -->

**Core dependencies**:
- **Web framework**: `axum` 0.8 + `tower` 0.5
- **Async runtime**: `tokio` 1 (full features)
- **ORM**: `sea-orm` 1
- **Task queue**: `fang` (async PostgreSQL)
- **Database**: SQLite or PostgreSQL (feature-gated), API 与队列分离
- **Serialization**: `serde` 1 + `serde_json` 1
- **Auth**: `jsonwebtoken` 9 + `argon2` 0.5
- **HTTP client**: `reqwest` 0.13
- **Image processing**: `image` 0.25 + `rayon` 1.12.0
- **Cloud storage**: AWS SDK S3 (`aws-sdk-s3` 1)
- **Logging**: `tracing` 0.1 + `tracing-subscriber` 0.3

**Dev dependencies**:
- `reqwest`, `serde_json`, `tokio`, `sea-orm`, `tower`, `http-body-util`

## Workflows [CONFIDENCE: high]

<!-- Source: README.md, CLAUDE.md -->

**Build & Run**:
```bash
# Default: SQLite API + PostgreSQL queue
cargo build

# No PostgreSQL (SQLite queue)
cargo build --no-default-features --features db-sqlite,queue-sqlite

# Production: PostgreSQL API + PostgreSQL queue
cargo build --no-default-features --features db-postgres,queue-postgres

# Run server
cargo run -p randimg-server

# Run worker (headless, no Axum)
cargo run -p randimg-worker

# Create admin user
cargo run -p randimg-server --bin create-admin

# Run color extraction benchmark
cargo run -p randimg-core --example color_extract_demo -- path/to/image.jpg
```

**Development workflow**:
1. Copy `.env.example` to `.env`
2. Set `SECRET_KEY` (must change from default)
3. Ensure PostgreSQL is running (or use `queue-sqlite` feature)
4. Run `cargo build` to compile
5. Run `cargo test -p randimg-core --features db-sqlite,queue-sqlite -- --skip color_test` to verify
6. Run `cargo run -p randimg-server` to start server

**Database migrations**:
- Migrations in `migration/src/` with `m{YYYYMMDD}_{seq}_{name}.rs` naming
- Auto-executed on startup
- Dual-database support via feature flags

## Common Tasks [CONFIDENCE: high]

<!-- Source: CLAUDE.md -->

**Adding new task types**:
1. Define job struct in `task_queue/jobs.rs` implementing `AsyncRunnable`
2. Add handler in `task_queue/handlers.rs`
3. Register worker in `lib.rs::spawn_workers()`

**Adding new routes**:
1. Create handler in `handlers/` module
2. Export `routes()` function returning Axum Router
3. Add `.merge(module::routes())` in `main.rs`

**Adding new database queries**:
1. Create/update file in `db/query/`
2. Use SeaORM `Select`, `Insert`, `Update`, `Delete` operations
3. Batch-fetch associations to avoid N+1 queries

## Pitfalls [CONFIDENCE: high]

<!-- Source: CLAUDE.md -->

**Database feature flags**:
- `db-sqlite` and `db-postgres` features are mutually exclusive
- `queue-sqlite` and `queue-postgres` features are mutually exclusive
- Cannot enable both simultaneously

**Database separation**:
- `API_DATABASE_URL` — SeaORM manages business data + `tasks` table
- `QUEUE_DATABASE_URL` — Fang manages task scheduling (PostgreSQL)
- Both must be configured when using `queue-postgres`

**Color worker process isolation**:
- In-process (default): color-extract runs as Fang worker
- Separate process: set `COLOR_WORKER_STANDALONE=true`

## Don'ts [CONFIDENCE: high]

<!-- Source: CLAUDE.md -->

- **Don't use `println!`** — Use `tracing` macros instead
- **Don't return raw `StatusCode`** — Use `AppError` variants
- **Don't commit `.env` files** — Contains secrets
- **Don't use default `SECRET_KEY`** — App panics at startup
- **Don't enable both `sqlite` and `postgres` features** — They're mutually exclusive
- **Don't block async runtime** — Use `spawn_blocking` for CPU-intensive work
- **Don't ignore N+1 queries** — Batch-fetch associations

## Recipes [CONFIDENCE: high]

<!-- Source: CLAUDE.md, README.md -->

**Add a new API route**:
```rust
// 1. Create handler in handlers/my_handler.rs
pub fn routes() -> Router<Arc<WorkerState>> {
    Router::new().route("/my-endpoint", get(my_handler))
}

// 2. Register in crates/randimg-server/src/main.rs
.merge(my_handler::routes())
```

**Add a new task type**:
```rust
// 1. Define job in task_queue/jobs.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyJob {
    pub data: String,
    pub parent_job_id: Option<String>,
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for MyJob {
    async fn run(&self, ctx: JobContext) -> Result<(), Error> {
        // Implementation
    }
}

// 2. Add handler in task_queue/handlers.rs
pub async fn handle_my_job(job: MyJob, ctx: JobContext) -> Result<(), Error> {
    // Implementation
}

// 3. Register worker in lib.rs
spawn_worker!(my_job_worker, handle_my_job);
```

**Run database migration**:
```bash
# Migrations auto-run on startup
# Manual migration (if needed):
cargo run --bin migrate
```

## Infrastructure [CONFIDENCE: medium]

<!-- Source: README.md, .env.example -->

**Database**:
- SQLite (development): `sqlite://data/randimg.db?mode=rwc`
- PostgreSQL (production): Connection string in `DATABASE_URL`

**Queue**:
- PostgreSQL (default): `QUEUE_DATABASE_URL` connection string
- SQLite (dev fallback): Same as API database

**Cloud storage**:
- DogeCloud (AWS S3 compatible)
- Requires: `DOGECLOUD_ACCESS_KEY`, `DOGECLOUD_SECRET_KEY`, `DOGECLOUD_S3_BUCKET`, `DOGECLOUD_S3_ENDPOINT`

**Pixiv API**:
- Requires: `PIXIV_REFRESH_TOKEN`
- Optional proxy: `PIXIV_PROXY`

**Server**:
- TCP: `0.0.0.0:8000`
- Unix socket: `unix:///run/randimg.sock`

## Deployment [CONFIDENCE: low]

<!-- Source: README.md -->

[OPTIONAL: Deployment information not found in project files]

**What we know**:
- Supports TCP and Unix socket listening
- Graceful shutdown via SIGINT/SIGTERM
- Database migrations auto-run on startup

**What we need**:
- Deployment scripts
- Docker configuration
- CI/CD pipeline
- Production environment setup

## Environment [CONFIDENCE: high]

<!-- Source: .env.example, README.md -->

**Required variables**:
- `SECRET_KEY` — JWT signing secret (must change from default)
- `PIXIV_REFRESH_TOKEN` — Required for Pixiv crawling

**Optional variables**:
- `API_DATABASE_URL` — Default: `sqlite://data/randimg.db?mode=rwc`
- `QUEUE_DATABASE_URL` — PostgreSQL connection string for Fang queue
- `JWT_EXPIRE_MINUTES` — Default: 60
- `CDN_BASE_URL` — Default: `https://cdn.example.com/`
- `IMAGE_DIR` — Default: `./images`
- `SERVER_ADDR` — Default: `0.0.0.0:8000`
- `PIXIV_PROXY` — Optional proxy for Pixiv API
- `RUST_LOG` — Default: `randimg_core=info,tower_http=info`
- `LOG_DIR` — Default: `./logs`
- `LOG_JSON` — Default: `false`
- `MAX_DISCOVER_HOPS` — Default: 3
- `DISCOVER_SEED_LIMIT` — Default: 5
- `DOGECLOUD_ACCESS_KEY` — For OSS upload
- `DOGECLOUD_SECRET_KEY` — For OSS upload
- `DOGECLOUD_S3_BUCKET` — For OSS upload
- `DOGECLOUD_S3_ENDPOINT` — For OSS upload
- `TASK_MAX_RETRIES` — Default: 3
- `TASK_BACKOFF_BASE` — Default: 2
- `TASK_POLL_INTERVAL_MS` — Default: 500
- `TASK_DEFAULT_TIMEOUT_SECS` — Default: 300
- `TASK_CONCURRENCY_CRAWL` — Default: 2
- `TASK_CONCURRENCY_DOWNLOAD` — Default: 4
- `TASK_CONCURRENCY_COLOR_EXTRACT` — Default: 2
- `TASK_CONCURRENCY_UPLOAD` — Default: 2
- `TASK_CONCURRENCY_ACCESSIBILITY_CHECK` — Default: 2
- `TASK_CONCURRENCY_DISCOVER` — Default: 1
- `TASK_CONCURRENCY_REFRESH_PIXIV_TOKEN` — Default: 1

## UI/UX [CONFIDENCE: low]

<!-- Source: README.md -->

[OPTIONAL: UI/UX information not found in project files]

**What we know**:
- API-only backend (no frontend code)
- JSON responses
- Image serving via CDN

**What we need**:
- Frontend repository
- UI/UX guidelines
- Design system documentation

## Locally defined skills

[OPTIONAL: No locally defined skills found in project]

**Searched locations**:
- `.opencode/skills/*.md` — Not found
- `.claude/skills/*.md` — Not found
- `.agents/skills/*.md` — Not found
- `AGENTS/skills/*.md` — Not found

## Related Knowledge Bases

- `CLAUDE.md` — Claude Code guidance (exists in project)
- `README.md` — Project documentation (exists in project)
- `.env.example` — Environment variable documentation (exists in project)

## Next Steps

1. **High priority**: Gather deployment information (Docker, CI/CD, production setup)
2. **Medium priority**: Document infrastructure details (monitoring, logging, scaling)
3. **Low priority**: Add UI/UX guidelines if frontend exists

## Changelog

- 2026-06-03: Initial knowledge base creation
- 2026-06-03: Major refactor — 3-crate workspace, Redis queue backend, WorkerState/ServerState split, distributed worker binary
- 2026-06-03: Queue migration — Apalis → Fang (async PostgreSQL), API/队列数据库分离, removed Redis dependency
