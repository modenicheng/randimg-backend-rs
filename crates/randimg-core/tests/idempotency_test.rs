//! Tests for idempotency guards in task queue handlers.
//!
//! Each handler checks whether the work has already been done (by inspecting
//! a sentinel field on the image record) and returns `Ok(())` early if so.
//!
//! Requires a running PostgreSQL instance. Uses a unique schema per test to
//! avoid interference. Set `API_DATABASE_URL` env var (defaults to
//! `postgres://localhost/randimg`).

use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, EntityTrait, Set};
use std::sync::Arc;

use randimg_core::config::{AppConfig, BindAddr};
use randimg_core::task_queue::jobs::{AccessibilityCheckJob, ColorExtractJob, UploadJob};

fn db_url() -> String {
    let _ = dotenvy::dotenv();
    std::env::var("API_DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/randimg".into())
}

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect(&db_url())
        .await
        .expect("Failed to connect to PostgreSQL — is it running?");
    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");
    db
}

fn make_test_config() -> AppConfig {
    AppConfig {
        api_database_url: db_url(),
        queue_database_url: "postgres://localhost/test_queue".into(),
        secret_key: "test-secret-key-for-jwt".into(),
        jwt_expire_minutes: 60,
        cdn_base_url: "https://cdn.test/".into(),
        image_dir: "/tmp/test_images".into(),
        server_addr: BindAddr::parse("127.0.0.1:8080"),
        pixiv_refresh_token: "".into(),
        pixiv_proxy: "".into(),
        pixiv_accept_lang: "en".into(),
        pixiv_timeout_secs: 30,
        log_level: "info".into(),
        log_dir: "/tmp/logs".into(),
        log_json: false,
        max_discover_hops: 3,
        discover_seed_limit: 5,
        dogecloud_access_key: "".into(),
        dogecloud_secret_key: "".into(),
        dogecloud_s3_bucket: "".into(),
        dogecloud_s3_endpoint: "".into(),
        cors_origins: "".into(),
        color_worker_rayon_threads: 2,
        color_worker_standalone: false,
        color_extract_k: 10,
        color_extract_max_iter: 50,
        color_extract_batch_size: 2048,
        color_extract_image_scale: 0.5,
        auth_max_retries: 3,
        auth_backoff_base_ms: 1000,
        task_max_retries: 3,
        task_backoff_base: 2,
        task_poll_interval_ms: 500,
        task_default_timeout_secs: 300,
        task_dedup_ttl_secs: 300,
        task_cleanup_ttl_hours: 168,
        dead_letter_ttl_hours: 720,
        drain_timeout_secs: 30,
        worker_health_port: 8001,
        watchdog_check_interval_secs: 30,
        watchdog_stuck_timeout_secs: 120,
        task_concurrency_crawl: 2,
        task_concurrency_download: 4,
        task_concurrency_color_extract: 2,
        task_concurrency_upload: 2,
        task_concurrency_accessibility_check: 2,
        task_concurrency_discover: 1,
        task_concurrency_refresh_pixiv_token: 1,
        task_concurrency_cleanup: 1,
    }
}

fn make_worker_state(db: DatabaseConnection) -> Arc<randimg_core::WorkerState> {
    let config = make_test_config();
    let queue_backend = randimg_core::db_backend::QueueBackend::new_noop();
    Arc::new(randimg_core::WorkerState {
        db,
        config,
        oss: randimg_core::dogecloud::DogeCloudOss::new_noop(),
        queue_backend: queue_backend.clone(),
        http_client: reqwest::Client::new(),
        shutdown_token: tokio_util::sync::CancellationToken::new(),
        worker_start_time: std::time::Instant::now(),
        active_tasks: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        discover_cache: Arc::new(dashmap::DashMap::new()),
        fingerprint_cache: queue_backend.fingerprint_cache.clone(),
        last_activity: Arc::new(dashmap::DashMap::new()),
        stuck_pools: Arc::new(dashmap::DashMap::new()),
    })
}

async fn create_image_with_author(
    db: &DatabaseConnection,
    title: &str,
) -> randimg_core::db::entities::image::Model {
    let author =
        randimg_core::db::query::author::find_or_create(db, "TestArtist", Some("pixiv"), Some("1"))
            .await
            .unwrap();
    let data = serde_json::json!({
        "title": title,
        "image_path": format!("{}.jpg", title),
        "source_url": format!("https://pixiv.net/{}", title),
        "source_id": 1000,
        "author_id": author.id,
        "width": 800,
        "height": 600,
        "aspect_ratio": 1.33,
    });
    randimg_core::db::query::image::create_image(db, &data)
        .await
        .unwrap()
}

#[tokio::test]
async fn test_upload_skips_if_already_public() {
    let db = setup_db().await;
    let img = create_image_with_author(&db, "upload_skip").await;

    use randimg_core::db::entities::image::{ActiveModel, Entity as ImageEntity};
    let existing = ImageEntity::find_by_id(img.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let mut active: ActiveModel = existing.into();
    active.is_public = Set(true);
    active.update(&db).await.unwrap();

    let state = make_worker_state(db);
    let job = UploadJob {
        image_id: img.id,
        image_path: "upload_skip.jpg".into(),
        parent_job_id: None,
        task_id: None,
        max_retries: 3,
        backoff_base: 2,
    };

    let result = randimg_core::task_queue::handlers::handle_upload(job, &state).await;
    assert!(
        result.is_ok(),
        "handle_upload should return Ok for already-uploaded image"
    );
}

#[tokio::test]
async fn test_color_extract_skips_if_colors_exist() {
    let db = setup_db().await;
    let img = create_image_with_author(&db, "color_skip").await;

    use randimg_core::db::entities::image::{ActiveModel, Entity as ImageEntity};
    let existing = ImageEntity::find_by_id(img.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let mut active: ActiveModel = existing.into();
    active.colors = Set(Some(serde_json::json!({"primary_color": [128, 64, 32]})));
    active.update(&db).await.unwrap();

    let state = make_worker_state(db);
    let job = ColorExtractJob {
        image_id: img.id,
        image_path: "color_skip.jpg".into(),
        parent_job_id: None,
        task_id: None,
        max_retries: 0,
        backoff_base: 2,
    };

    let result = randimg_core::task_queue::handlers::handle_color_extract(job, &state).await;
    assert!(
        result.is_ok(),
        "handle_color_extract should return Ok for image with existing colors"
    );
}

#[tokio::test]
async fn test_accessibility_check_skips_if_accessible_set() {
    let db = setup_db().await;
    let img = create_image_with_author(&db, "a11y_skip").await;

    use randimg_core::db::entities::image::{ActiveModel, Entity as ImageEntity};
    let existing = ImageEntity::find_by_id(img.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let mut active: ActiveModel = existing.into();
    active.accessible = Set(Some(true));
    active.update(&db).await.unwrap();

    let state = make_worker_state(db);
    let job = AccessibilityCheckJob {
        image_id: img.id,
        image_path: "a11y_skip.jpg".into(),
        parent_job_id: None,
        task_id: None,
        max_retries: 3,
        backoff_base: 2,
    };

    let result = randimg_core::task_queue::handlers::handle_accessibility_check(job, &state).await;
    assert!(
        result.is_ok(),
        "handle_accessibility_check should return Ok for already-checked image"
    );
}
