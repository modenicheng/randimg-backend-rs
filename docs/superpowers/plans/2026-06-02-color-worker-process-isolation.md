# Color Worker Process Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Isolate the CPU-heavy color extraction (KMeans + rayon) into a separate worker process so it cannot block the main HTTP server's tokio runtime, and can fully utilize multi-core CPUs independently.

**Architecture:** Create a dedicated `color-worker` binary that initializes its own Apalis color-extract worker and rayon thread pool. The main binary optionally spawns it as a child process via `tokio::process::Command`. Both binaries share the same database — Apalis's storage backend handles task coordination via SQLite WAL locks (dev) or `SELECT ... FOR UPDATE` (prod). The color handler is also wrapped with `tokio::task::spawn_blocking` as a defense-in-depth measure so that even if the worker runs in-process, it won't block tokio's async executor.

**Tech Stack:** Apalis 1.0.0-rc.9, rayon, tokio (process + spawn_blocking), SeaORM, SQLite/PostgreSQL

---

## Research Summary: Apalis Process Capabilities

**Apalis does NOT support spawning workers as separate OS processes.** After examining `apalis-core-1.0.0-rc.9` source code:

- `Worker` is an in-process async type — `run()` returns a future polled on the current tokio runtime
- `Monitor` uses `join_all(workers).await` — no process boundary
- No `std::process::Command`, `tokio::process`, `fork()`, or IPC primitives anywhere in the crate
- The "distributed" model works via **shared storage backends**: multiple separate binaries each connect to the same SQLite/PostgreSQL database and coordinate through DB-level locking

This means the solution must be **self-managed process spawning** — either a separate binary deployed independently, or spawned as a child process from main.

---

## Problem Analysis

### Current State

```
┌──────────────────────────────────────┐
│          Main Process (tokio)        │
│                                      │
│  ┌─────────┐  ┌─────────┐           │
│  │ HTTP    │  │ crawl   │           │
│  │ Server  │  │ worker  │           │
│  └─────────┘  └─────────┘           │
│                                      │
│  ┌──────────────┐  ┌──────────┐     │
│  │color-extract │  │ download │     │
│  │  worker      │  │  worker  │     │
│  │  (rayon!!!)  │  │          │     │
│  └──────────────┘  └──────────┘     │
│                                      │
│  ┌─────────┐  ┌─────────┐           │
│  │ upload  │  │discover │           │
│  │ worker  │  │ worker  │           │
│  └─────────┘  └─────────┘           │
└──────────────────────────────────────┘
```

### Problems

1. **CPU contention**: `handle_color_extract` runs synchronous KMeans (50 iterations, rayon parallel chunks of 4096 pixels) directly on tokio's async worker threads. Rayon's work-stealing competes with tokio's scheduler for CPU time.

2. **Blocking the executor**: Even though Apalis spawns each job as a tokio task, CPU-bound work inside an async task blocks the thread until completion. With concurrency=2, two simultaneous color extractions can starve the tokio runtime.

3. **No CPU isolation**: The rayon global thread pool is shared across all in-process uses — download, upload, and HTTP handler threads all compete with KMeans for CPU cores.

### Target State

```
┌──────────────────────────┐     ┌──────────────────────────┐
│    Main Process (tokio)  │     │  Color Worker Process    │
│                          │     │                          │
│  ┌──────┐ ┌──────────┐  │     │  ┌────────────────────┐  │
│  │ HTTP │ │ crawl    │  │     │  │ color-extract      │  │
│  │Server│ │ download │  │     │  │ worker (own tokio) │  │
│  │      │ │ upload   │  │     │  │                    │  │
│  │      │ │ discover │  │     │  │ rayon pool (N cpu) │  │
│  └──────┘ └──────────┘  │     │  └────────────────────┘  │
│                          │     │                          │
│   No color-extract       │     │  No HTTP/crawl/download  │
│   worker here            │     │  workers here            │
└──────────┬───────────────┘     └──────────┬───────────────┘
           │                                │
           └────────┬───────────────────────┘
                    │
           ┌────────▼────────┐
           │   Shared DB     │
           │ (SQLite / PG)   │
           │                 │
           │  Apalis tables: │
           │  - Jobs         │
           │  - Workers      │
           └─────────────────┘
```

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `Cargo.toml` | Modify | Add `[[bin]]` entry for `color-worker` |
| `src/bin/color_worker.rs` | Create | Dedicated color extraction worker binary |
| `src/color/mod.rs` | Modify | Add `DedicatedPool` wrapper for rayon thread pool control |
| `src/task_queue/handlers.rs` | Modify | Wrap `handle_color_extract` body with `spawn_blocking` |
| `src/lib.rs` | Modify | Conditionally exclude color-extract worker from `spawn_workers()` |
| `src/main.rs` | Modify | Optionally spawn color-worker as child process |
| `src/config.rs` | Modify | Add `COLOR_WORKER_RAYON_THREADS` and `COLOR_WORKER_STANDALONE` env vars |

---

### Task 1: Add `COLOR_WORKER_RAYON_THREADS` config

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add new config fields**

Read `src/config.rs` first, then add to the `AppConfig` struct:

```rust
/// Number of threads for the color extraction rayon pool.
/// Defaults to number of CPUs. Set to limit CPU usage of color extraction.
pub color_worker_rayon_threads: usize,

/// If true, the main binary will NOT spawn a color-extract worker.
/// Use when running color-worker as a separate process.
pub color_worker_standalone: bool,
```

Add to `from_env()`:

```rust
color_worker_rayon_threads: std::env::var("COLOR_WORKER_RAYON_THREADS")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or_else(|| std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)),
color_worker_standalone: std::env::var("COLOR_WORKER_STANDALONE")
    .map(|v| v == "1" || v == "true")
    .unwrap_or(false),
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add color_worker_rayon_threads and color_worker_standalone env vars"
```

---

### Task 2: Add dedicated rayon pool wrapper in `color/mod.rs`

**Files:**
- Modify: `src/color/mod.rs`

This creates a dedicated rayon `ThreadPool` for color extraction so it doesn't share the global rayon pool. The thread count is configurable via env var.

- [ ] **Step 1: Add `DedicatedPool` to `src/color/mod.rs`**

At the top of `src/color/mod.rs`, add after the existing imports:

```rust
use std::sync::OnceLock;

/// Dedicated rayon thread pool for color extraction.
/// Initialized once with the configured number of threads.
static COLOR_POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();

/// Initialize (or return existing) dedicated color extraction thread pool.
///
/// `threads`: number of threads in the pool. Panics if called with different
/// values after initialization (OnceLock semantics).
pub fn init_color_pool(threads: usize) -> &'static rayon::ThreadPool {
    COLOR_POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|idx| format!("color-worker-{}", idx))
            .build()
            .expect("Failed to create color extraction rayon pool")
    })
}

/// Run a closure on the dedicated color extraction thread pool.
///
/// This ensures color extraction work runs on isolated threads that don't
/// compete with tokio's async worker threads or the global rayon pool.
pub fn run_on_color_pool<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    let pool = init_color_pool(
        std::env::var("COLOR_WORKER_RAYON_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)),
    );
    pool.install(f)
}
```

- [ ] **Step 2: Update `extract_theme_colors` to use the dedicated pool**

Replace the existing `extract_theme_colors` function body. The key change: wrap the entire function body in `run_on_color_pool` so that all rayon operations (`par_iter`, `par_chunks`, etc.) use the dedicated pool instead of the global one.

```rust
pub fn extract_theme_colors(img: &DynamicImage) -> ThemeColors {
    run_on_color_pool(|| {
        // Scale down to reduce computation
        let scale = 0.5;
        let (w, h) = img.dimensions();
        let new_w = ((w as f64 * scale) as u32).max(1);
        let new_h = ((h as f64 * scale) as u32).max(1);
        let small = img.resize_exact(new_w, new_h, image::imageops::FilterType::Nearest);
        let rgb = small.to_rgb8();

        // Collect pixels as [u8; 3]
        let pixels: Vec<[u8; 3]> = rgb.pixels().map(|p| [p[0], p[1], p[2]]).collect();

        if pixels.is_empty() {
            return ThemeColors {
                primary_color: [0, 0, 0],
                primary_lab: [0.0; 3],
                colors: vec![[0, 0, 0]; 10],
                colors_lab: vec![[0.0; 3]; 10],
            };
        }

        // Primary color from histogram (16 levels per channel)
        let primary_color = histogram_primary_color(&pixels, 16);
        let primary_lab = rgb_to_lab(primary_color[0], primary_color[1], primary_color[2]);

        // Convert to LAB for clustering (parallel on dedicated pool)
        let lab_pixels: Vec<[f64; 3]> = pixels
            .par_iter()
            .map(|p| rgb_to_lab(p[0], p[1], p[2]))
            .collect();

        // KMeans clustering in LAB space
        let lab_centroids = kmeans::kmeans(&lab_pixels, 10, 50, Some(2048));

        // Sort by L* (lightness)
        let mut sorted_lab = lab_centroids;
        sorted_lab.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));

        // Keep LAB centroids for storage, also convert to RGB
        let colors_lab: Vec<[f64; 3]> = sorted_lab.clone();
        let colors: Vec<[u8; 3]> = sorted_lab
            .into_iter()
            .map(|c| lab_to_rgb(c[0], c[1], c[2]))
            .collect();

        ThemeColors {
            primary_color,
            primary_lab,
            colors,
            colors_lab,
        }
    })
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 4: Run the color extraction benchmark to verify no regression**

Run: `cargo run --example color_extract_demo -- tests/assets/test_image.jpg`
Expected: Similar performance numbers to before (the dedicated pool should perform identically to the global pool for this workload).

- [ ] **Step 5: Commit**

```bash
git add src/color/mod.rs
git commit -m "feat(color): use dedicated rayon thread pool for color extraction"
```

---

### Task 3: Wrap `handle_color_extract` with `spawn_blocking`

**Files:**
- Modify: `src/task_queue/handlers.rs`

This is defense-in-depth: even if the color worker runs in-process (when `COLOR_WORKER_STANDALONE=false`), `spawn_blocking` ensures the CPU-heavy work runs on tokio's blocking thread pool, not on the async worker threads.

- [ ] **Step 1: Refactor `handle_color_extract`**

Replace the existing `handle_color_extract` function in `src/task_queue/handlers.rs`:

```rust
/// Extract color palette from a downloaded image.
///
/// The heavy computation (image decode + KMeans) is offloaded to
/// `spawn_blocking` so it runs on tokio's blocking thread pool,
/// not on the async worker threads. Combined with the dedicated
/// rayon pool inside `extract_theme_colors`, this ensures color
/// extraction never blocks the async runtime.
pub async fn handle_color_extract(
    job: ColorExtractJob,
    _task_id: TaskId<Ulid>,
    state: Data<Arc<AppState>>,
) -> Result<(), BoxDynError> {
    let file_path = format!("{}/{}", state.config.image_dir, job.image_path);
    let image_dir = state.config.image_dir.clone();

    // CPU-heavy work: image decode + KMeans on blocking thread pool
    let (colors, img_bytes) = tokio::task::spawn_blocking(move || {
        let full_path = format!("{}/{}", image_dir, job.image_path);
        let img = ::image::open(&full_path)
            .map_err(|e| format!("Failed to open image: {}", e))?;
        let colors = crate::color::extract_theme_colors(&img);
        Ok::<_, String>((colors, job.image_path))
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    // DB writes stay on the async runtime (they are I/O-bound)
    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img_model) = Image::find_by_id(job.image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        let mut active: image::ActiveModel = img_model.into();
        active.colors = Set(Some(serde_json::to_value(&colors).unwrap()));
        active.primary_l = Set(Some(colors.primary_lab[0]));
        active.primary_a = Set(Some(colors.primary_lab[1]));
        active.primary_b = Set(Some(colors.primary_lab[2]));
        active.update(&state.db).await.map_err(|e| e.to_string())?;
    }

    // Upsert palette entries
    use crate::db::entities::image_color_palette::{self, Entity as PaletteEntity};

    PaletteEntity::delete_many()
        .filter(image_color_palette::Column::ImageId.eq(job.image_id))
        .exec(&state.db)
        .await
        .map_err(|e| format!("Failed to clear old palette: {}", e))?;

    for (i, (rgb, lab)) in colors
        .colors
        .iter()
        .zip(colors.colors_lab.iter())
        .enumerate()
    {
        let entry = image_color_palette::ActiveModel {
            id: sea_orm::NotSet,
            image_id: Set(job.image_id),
            color_index: Set(i as i32),
            rgb_r: Set(rgb[0] as i32),
            rgb_g: Set(rgb[1] as i32),
            rgb_b: Set(rgb[2] as i32),
            lab_l: Set(lab[0]),
            lab_a: Set(lab[1]),
            lab_b: Set(lab[2]),
        };
        entry
            .insert(&state.db)
            .await
            .map_err(|e| format!("Failed to insert palette entry: {}", e))?;
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/task_queue/handlers.rs
git commit -m "feat(handlers): wrap color_extract with spawn_blocking for async safety"
```

---

### Task 4: Create the `color-worker` binary

**Files:**
- Modify: `Cargo.toml`
- Create: `src/bin/color_worker.rs`

This binary runs only the color extraction worker. It connects to the same database as the main process, initializes its own Apalis worker with its own rayon thread pool, and processes color_extract jobs independently.

- [ ] **Step 1: Add `[[bin]]` entry to `Cargo.toml`**

After the existing `[[bin]]` for `create_admin`, add:

```toml
[[bin]]
name = "color-worker"
path = "src/bin/color_worker.rs"
```

- [ ] **Step 2: Create `src/bin/color_worker.rs`**

```rust
//! Standalone color extraction worker process.
//!
//! This binary runs only the color_extract Apalis worker, connecting to the
//! same database as the main server. It uses its own tokio runtime and a
//! dedicated rayon thread pool to avoid competing with the main process for
//! CPU resources.
//!
//! ## Usage
//!
//! ```bash
//! # Run as a separate process (recommended for production)
//! COLOR_WORKER_RAYON_THREADS=4 cargo run --bin color-worker
//!
//! # Or with the release binary
//! COLOR_WORKER_RAYON_THREADS=4 ./target/release/color-worker
//! ```
//!
//! ## Environment Variables
//!
//! - `DATABASE_URL` — same as the main server
//! - `COLOR_WORKER_RAYON_THREADS` — rayon thread count (default: CPU count)
//! - `LOG_LEVEL` — tracing filter (default: `info`)

use apalis::prelude::*;
use apalis::layers::retry::RetryPolicy;
use randimg_backend_rs::config::AppConfig;
use randimg_backend_rs::db_backend::JobStorage;
use randimg_backend_rs::task_queue::handlers::handle_color_extract;
use randimg_backend_rs::task_queue::jobs::ColorExtractJob;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    // Logging setup
    let log_level = std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level)),
        )
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(true)
        .compact()
        .init();

    let config = AppConfig::from_env();

    // Initialize the dedicated rayon pool BEFORE connecting to DB,
    // so the pool is ready when the first job arrives.
    let rayon_threads = config.color_worker_rayon_threads;
    randimg_backend_rs::color::init_color_pool(rayon_threads);
    tracing::info!(threads = rayon_threads, "Initialized color extraction rayon pool");

    // Connect to the same database as the main server
    let (apalis_pool, job_storage) = randimg_backend_rs::db_backend::init(&config.database_url)
        .await
        .expect("Failed to initialize Apalis job queue");

    // Build a minimal AppState (only what handle_color_extract needs: db + config)
    let db = randimg_backend_rs::db::init_database(&config.database_url).await;

    let state = Arc::new(randimg_backend_rs::AppState {
        db,
        config: config.clone(),
        oss: randimg_backend_rs::dogecloud::DogeCloudOss::new(
            &config,
            reqwest::Client::new(),
        ),
        job_storage: job_storage.clone(),
        apalis_pool: apalis_pool.clone(),
        http_client: reqwest::Client::new(),
        worker_handles: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    });

    // Build the color-extract worker
    let storage = job_storage.color_extract.lock().await.clone();

    tracing::info!("Starting color-worker process");

    let worker = WorkerBuilder::new("color-extract")
        .backend(storage)
        .data(Arc::new(state) as std::sync::Arc<dyn std::any::Any + Send + Sync>)
        .concurrency(2)
        .retry(RetryPolicy::retries(3))
        .enable_tracing()
        .build(handle_color_extract);

    // Run until shutdown signal
    tokio::select! {
        result = worker.run() => {
            match result {
                Ok(()) => tracing::warn!("Color worker exited unexpectedly"),
                Err(e) => tracing::error!(error = %e, "Color worker exited with error"),
            }
        }
        _ = shutdown_signal() => {
            tracing::info!("Received shutdown signal, stopping color worker");
        }
    }
}

async fn shutdown_signal() {
    use tokio::signal;
    let ctrl_c = signal::ctrl_c();
    let sigterm = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => tracing::info!("Received SIGINT"),
        _ = sigterm => tracing::info!("Received SIGTERM"),
    }
}
```

**Note:** The `WorkerBuilder::data()` call expects concrete types. The exact API may need adjustment based on how `handle_color_extract`'s signature resolves its `Data<Arc<AppState>>` parameter. The key pattern is: clone the storage from `JobStorage`, build a worker with `WorkerBuilder`, and call `.run().await`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --bin color-worker`
Expected: Compiles without errors. Fix any type mismatches in the `WorkerBuilder` chain.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/bin/color_worker.rs
git commit -m "feat: add standalone color-worker binary for process isolation"
```

---

### Task 5: Conditionally exclude color-extract from main's `spawn_workers`

**Files:**
- Modify: `src/lib.rs`

When `COLOR_WORKER_STANDALONE=true`, the main binary should not spawn a color-extract worker — that's handled by the separate `color-worker` process.

- [ ] **Step 1: Read `src/lib.rs` and modify `spawn_workers`**

In the `spawn_workers` function, change the color-extract worker spawn to be conditional:

```rust
// In the handles vec initialization:
let mut handles = vec![
    spawn_worker!("crawl", js.crawl, handle_crawl, 2),
    spawn_worker!("download", js.download, handle_download, 4),
    // color-extract worker: skip if running as standalone process
    if !state.config.color_worker_standalone {
        spawn_worker!("color-extract", js.color_extract, handle_color_extract, 2)
    } else {
        tracing::info!("Skipping color-extract worker (COLOR_WORKER_STANDALONE=true)");
        tokio::spawn(async { std::future::pending::<()>().await })
    },
    spawn_worker!("upload", js.upload, handle_upload, 2),
    spawn_worker!(
        "accessibility-check",
        js.accessibility_check,
        handle_accessibility_check,
        2
    ),
    spawn_worker!("discover", js.discover, handle_discover, 1),
    spawn_worker!(
        "refresh-pixiv-token",
        js.refresh_pixiv_token,
        handle_refresh_pixiv_token,
        1
    ),
];
```

The `tokio::spawn(async { std::future::pending().await })` creates a handle that never resolves — it's a no-op placeholder so the vec type stays uniform.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/lib.rs
git commit -m "feat(lib): conditionally skip color-extract worker when COLOR_WORKER_STANDALONE=true"
```

---

### Task 6: (Optional) Auto-spawn color-worker from main

**Files:**
- Modify: `src/main.rs`

This task is optional. It makes the main binary automatically spawn `color-worker` as a child process when `COLOR_WORKER_STANDALONE=true` and `COLOR_WORKER_AUTO_SPAWN=true`. This is convenient for development — in production, you'd typically manage both processes with systemd/supervisor.

- [ ] **Step 1: Add child process spawning logic to `main.rs`**

After the `let worker_handles = ...` line in `main()`, add:

```rust
// Optionally spawn color-worker as a child process
let _color_worker_child: Option<tokio::process::Child> = if config.color_worker_standalone
    && std::env::var("COLOR_WORKER_AUTO_SPAWN")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false)
{
    tracing::info!("Spawning color-worker as child process");
    let child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .arg("--bin")
        .arg("color-worker")
        .env("DATABASE_URL", &config.database_url)
        .env(
            "COLOR_WORKER_RAYON_THREADS",
            config.color_worker_rayon_threads.to_string(),
        )
        .env("LOG_LEVEL", &config.log_level)
        .env("SECRET_KEY", &config.secret_key)
        .spawn()
        .expect("Failed to spawn color-worker child process");
    tracing::info!(pid = ?child.id(), "Color-worker child process spawned");
    Some(child)
} else {
    None
};
```

Also add cleanup in the shutdown section (before the existing shutdown log):

```rust
// Shut down color-worker child process if spawned
if let Some(mut child) = _color_worker_child {
    tracing::info!("Sending SIGTERM to color-worker child process");
    // On Unix, the child gets SIGTERM when the parent exits naturally,
    // but we can be explicit:
    child.kill().await.ok();
    let _ = child.wait().await;
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(main): optionally auto-spawn color-worker as child process"
```

---

### Task 7: Update `.env.example` and verify end-to-end

**Files:**
- Modify: `.env.example`

- [ ] **Step 1: Add new env vars to `.env.example`**

Append to `.env.example`:

```bash
# ── Color Worker Process Isolation ──────────────────────────
# Set to "true" to run color extraction in a separate binary.
# The main server will NOT spawn a color-extract worker.
# Run `cargo run --bin color-worker` as a separate process.
# COLOR_WORKER_STANDALONE=false

# Number of rayon threads for color extraction (default: CPU count).
# Lower this to limit CPU usage, raise it to utilize more cores.
# COLOR_WORKER_RAYON_THREADS=4

# Auto-spawn color-worker as a child process from main.
# Only effective when COLOR_WORKER_STANDALONE=true.
# COLOR_WORKER_AUTO_SPAWN=true
```

- [ ] **Step 2: Full build check**

Run: `cargo build`
Expected: Both `randimg-backend-rs` and `color-worker` binaries build successfully.

- [ ] **Step 3: Verify color-worker binary exists**

Run: `cargo run --bin color-worker -- --help 2>&1 || true`
Expected: The binary starts and attempts to connect to the database (it will fail without a running DB, which is expected).

- [ ] **Step 4: Run existing tests**

Run: `cargo test`
Expected: All existing tests pass — the color extraction logic is unchanged, only the execution context differs.

- [ ] **Step 5: Commit**

```bash
git add .env.example
git commit -m "docs(env): add color worker configuration env vars"
```

---

### Task 8: Update CLAUDE.md documentation

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Add color worker documentation**

Add a new section after the "Task queue" section in CLAUDE.md:

```markdown
#### Color worker process isolation

The color extraction worker (KMeans + rayon) is CPU-intensive. Two modes:

1. **In-process** (default): color-extract runs as an Apalis worker inside the main binary. `spawn_blocking` + dedicated rayon pool prevent it from blocking the async runtime.

2. **Separate process** (`COLOR_WORKER_STANDALONE=true`): the main binary skips spawning the color-extract worker. Run `cargo run --bin color-worker` as a separate process. Both binaries connect to the same database; Apalis storage handles coordination.

Environment variables:
- `COLOR_WORKER_STANDALONE` — `true` to exclude color-extract from main binary
- `COLOR_WORKER_RAYON_THREADS` — rayon thread count for color extraction (default: CPU count)
- `COLOR_WORKER_AUTO_SPAWN` — `true` to auto-spawn color-worker as child process from main
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add color worker process isolation documentation"
```
