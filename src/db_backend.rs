//! Single point of backend abstraction for database storage and job queues.
//!
//! All `#[cfg(feature = "...")]` branching for database backend selection lives
//! here. The rest of the codebase uses the exported types (`Pool`, `JobStorage`,
//! `init()`) without caring which backend is active.

#[cfg(all(feature = "sqlite", feature = "postgres"))]
compile_error!("Features 'sqlite' and 'postgres' are mutually exclusive. Use --no-default-features when enabling postgres.");

use std::sync::Arc;

use apalis::prelude::*;
use sea_orm::DatabaseConnection;
use tokio::sync::Mutex;
use ulid::Ulid;

use crate::db::query;
use crate::task_queue::jobs::*;

// ---------------------------------------------------------------------------
// Pool type
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlite")]
pub type Pool = apalis_sqlite::SqlitePool;

#[cfg(feature = "postgres")]
pub type Pool = apalis_postgres::PgPool;

// ---------------------------------------------------------------------------
// Storage type (backend-specific internals behind a unified alias)
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlite")]
type Storage<T> = apalis_sqlite::SqliteStorage<
    T,
    apalis_codec::json::JsonCodec<apalis_sqlite::CompactType>,
    apalis_sqlite::fetcher::SqliteFetcher,
>;

#[cfg(feature = "postgres")]
type Storage<T> = apalis_postgres::PostgresStorage<T>;

// ---------------------------------------------------------------------------
// JobStorage
// ---------------------------------------------------------------------------

/// Holds all typed job storages. Each storage is mutex-wrapped because
/// `TaskSink::push` requires `&mut self`.
#[derive(Clone)]
pub struct JobStorage {
    pub crawl: Arc<Mutex<Storage<CrawlJob>>>,
    pub download: Arc<Mutex<Storage<DownloadJob>>>,
    pub color_extract: Arc<Mutex<Storage<ColorExtractJob>>>,
    pub upload: Arc<Mutex<Storage<UploadJob>>>,
    pub accessibility_check: Arc<Mutex<Storage<AccessibilityCheckJob>>>,
    pub discover: Arc<Mutex<Storage<DiscoverJob>>>,
    pub refresh_pixiv_token: Arc<Mutex<Storage<RefreshPixivTokenJob>>>,
}

impl JobStorage {
    /// Create a new `JobStorage` from a connected pool.
    ///
    /// The pool's setup/migrations must have been run beforehand (see [`init`]).
    pub fn new(pool: &Pool) -> Self {
        Self {
            crawl: Arc::new(Mutex::new(new_storage(pool))),
            download: Arc::new(Mutex::new(new_storage(pool))),
            color_extract: Arc::new(Mutex::new(new_storage(pool))),
            upload: Arc::new(Mutex::new(new_storage(pool))),
            accessibility_check: Arc::new(Mutex::new(new_storage(pool))),
            discover: Arc::new(Mutex::new(new_storage(pool))),
            refresh_pixiv_token: Arc::new(Mutex::new(new_storage(pool))),
        }
    }

    /// Create a `JobStorage` backed by an in-memory SQLite pool.
    ///
    /// This is intended for tests that need a functional `AppState` without
    /// connecting to an external database. The pool is ephemeral and will be
    /// dropped when the returned `JobStorage` is dropped.
    #[cfg(feature = "sqlite")]
    pub async fn new_for_test() -> Self {
        let pool = apalis_sqlite::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to create in-memory SQLite pool for test");
        apalis_sqlite::SqliteStorage::setup(&pool)
            .await
            .expect("Failed to setup Apalis storage for test");
        Self::new(&pool)
    }

    /// Push a job to the crawl queue.
    pub async fn push_crawl(&self, job: CrawlJob) -> Result<(), String> {
        self.crawl.lock().await.push(job).await.map_err(|e| e.to_string())
    }

    /// Push a job to the download queue.
    pub async fn push_download(&self, job: DownloadJob) -> Result<(), String> {
        self.download.lock().await.push(job).await.map_err(|e| e.to_string())
    }

    /// Push a job to the color_extract queue.
    pub async fn push_color_extract(&self, job: ColorExtractJob) -> Result<(), String> {
        self.color_extract.lock().await.push(job).await.map_err(|e| e.to_string())
    }

    /// Push a job to the upload queue.
    pub async fn push_upload(&self, job: UploadJob) -> Result<(), String> {
        self.upload.lock().await.push(job).await.map_err(|e| e.to_string())
    }

    /// Push a job to the accessibility_check queue.
    pub async fn push_accessibility_check(&self, job: AccessibilityCheckJob) -> Result<(), String> {
        self.accessibility_check
            .lock()
            .await
            .push(job)
            .await
            .map_err(|e| e.to_string())
    }

    /// Push a job to the discover queue.
    pub async fn push_discover(&self, job: DiscoverJob) -> Result<(), String> {
        self.discover.lock().await.push(job).await.map_err(|e| e.to_string())
    }

    /// Push a job to the refresh_pixiv_token queue.
    pub async fn push_refresh_pixiv_token(&self, job: RefreshPixivTokenJob) -> Result<(), String> {
        self.refresh_pixiv_token
            .lock()
            .await
            .push(job)
            .await
            .map_err(|e| e.to_string())
    }

    // ── Push-with-parent variants ────────────────────────────────────────
    //
    // These use `TaskBuilder::with_task_id` to pre-generate a ULID, push via
    // `push_task`, then immediately record the parent-child relationship in
    // `task_dependencies`.  This ensures the hierarchy is visible *before*
    // the child job starts executing, fixing the bug where pending children
    // appeared as root tasks in `GET /tasks/roots`.

    /// Push a download job and record its parent-child relationship.
    ///
    /// Returns the child job's ULID string.
    pub async fn push_download_with_parent(
        &self,
        job: DownloadJob,
        db: &DatabaseConnection,
    ) -> Result<String, String> {
        let child_id = Ulid::new();
        let parent_id = job.parent_job_id.clone();

        let task = TaskBuilder::new(job)
            .with_task_id(TaskId::new(child_id))
            .build();

        self.download
            .lock()
            .await
            .push_task(task)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(pid) = parent_id {
            query::task_dependency::record(db, &pid, &child_id.to_string())
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(child_id.to_string())
    }

    /// Push a color_extract job and record its parent-child relationship.
    pub async fn push_color_extract_with_parent(
        &self,
        job: ColorExtractJob,
        db: &DatabaseConnection,
    ) -> Result<String, String> {
        let child_id = Ulid::new();
        let parent_id = job.parent_job_id.clone();

        let task = TaskBuilder::new(job)
            .with_task_id(TaskId::new(child_id))
            .build();

        self.color_extract
            .lock()
            .await
            .push_task(task)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(pid) = parent_id {
            query::task_dependency::record(db, &pid, &child_id.to_string())
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(child_id.to_string())
    }

    /// Push an upload job and record its parent-child relationship.
    pub async fn push_upload_with_parent(
        &self,
        job: UploadJob,
        db: &DatabaseConnection,
    ) -> Result<String, String> {
        let child_id = Ulid::new();
        let parent_id = job.parent_job_id.clone();

        let task = TaskBuilder::new(job)
            .with_task_id(TaskId::new(child_id))
            .build();

        self.upload
            .lock()
            .await
            .push_task(task)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(pid) = parent_id {
            query::task_dependency::record(db, &pid, &child_id.to_string())
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(child_id.to_string())
    }

    /// Push an accessibility_check job and record its parent-child relationship.
    pub async fn push_accessibility_check_with_parent(
        &self,
        job: AccessibilityCheckJob,
        db: &DatabaseConnection,
    ) -> Result<String, String> {
        let child_id = Ulid::new();
        let parent_id = job.parent_job_id.clone();

        let task = TaskBuilder::new(job)
            .with_task_id(TaskId::new(child_id))
            .build();

        self.accessibility_check
            .lock()
            .await
            .push_task(task)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(pid) = parent_id {
            query::task_dependency::record(db, &pid, &child_id.to_string())
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(child_id.to_string())
    }

    /// Push a discover job and record its parent-child relationship.
    pub async fn push_discover_with_parent(
        &self,
        job: DiscoverJob,
        db: &DatabaseConnection,
    ) -> Result<String, String> {
        let child_id = Ulid::new();
        let parent_id = job.parent_job_id.clone();

        let task = TaskBuilder::new(job)
            .with_task_id(TaskId::new(child_id))
            .build();

        self.discover
            .lock()
            .await
            .push_task(task)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(pid) = parent_id {
            query::task_dependency::record(db, &pid, &child_id.to_string())
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(child_id.to_string())
    }
}

// ---------------------------------------------------------------------------
// Backend-specific constructor (only cfg-gated call site for Storage<T>)
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlite")]
fn new_storage<T>(pool: &Pool) -> Storage<T> {
    apalis_sqlite::SqliteStorage::new(pool)
}

#[cfg(feature = "postgres")]
fn new_storage<T>(pool: &Pool) -> Storage<T> {
    apalis_postgres::PostgresStorage::new(pool)
}

// ---------------------------------------------------------------------------
// init — connect pool, run setup migrations, return (Pool, JobStorage)
// ---------------------------------------------------------------------------

/// Connect to the database, run Apalis setup migrations, and build a
/// [`JobStorage`].
///
/// # Errors
/// Returns an error string if the connection or migration fails.
#[cfg(feature = "sqlite")]
pub async fn init(database_url: &str) -> Result<(Pool, JobStorage), String> {
    let pool = apalis_sqlite::SqlitePool::connect(database_url)
        .await
        .map_err(|e| e.to_string())?;

    // Enable WAL journal mode and set busy_timeout for better concurrent access.
    // This prevents SQLITE_BUSY errors when multiple workers write simultaneously.
    sqlx::raw_sql("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
        .execute(&pool)
        .await
        .map_err(|e| format!("Failed to set SQLite pragmas: {}", e))?;

    apalis_sqlite::SqliteStorage::setup(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let job_storage = JobStorage::new(&pool);
    Ok((pool, job_storage))
}

#[cfg(feature = "postgres")]
pub async fn init(database_url: &str) -> Result<(Pool, JobStorage), String> {
    let pool = apalis_postgres::PgPool::connect(database_url)
        .await
        .map_err(|e| e.to_string())?;
    apalis_postgres::PostgresStorage::setup(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let job_storage = JobStorage::new(&pool);
    Ok((pool, job_storage))
}
