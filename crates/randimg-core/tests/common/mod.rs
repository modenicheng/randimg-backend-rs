//! Shared test utilities for handler integration tests.
//!
//! All handler-level integration tests use in-memory SQLite with real
//! SeaORM migrations — no external server or database needed.

#![cfg(feature = "http")]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt;
use migration::MigratorTrait;
use sea_orm::{Database, DatabaseConnection};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

use randimg_core::auth::jwt;
use randimg_core::auth::password;
use randimg_core::config::{AppConfig, BindAddr};
use randimg_core::db_backend;
use randimg_core::db::query::admin;

/// Build a complete `AppConfig` with test-safe values.
///
/// All pointers (URLs, dirs) point to non-production locations.
/// `secret_key` is fixed at `"test-secret-key-for-jwt"` so test
/// tokens can be validated predictably.
pub fn make_test_config() -> AppConfig {
    AppConfig {
        api_database_url: "postgres://localhost/test_db".into(),
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
        auth_backoff_base_ms: 500,
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

/// Connect to an in-memory SQLite database and run all migrations.
pub async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory SQLite");
    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");
    db
}

/// Build an Axum Router with ALL handler routes merged in, backed by
/// an in-memory SQLite database and no-op queue / OSS backends.
///
/// The router is suitable for `tower::ServiceExt::oneshot` testing.
pub async fn build_test_router(db: DatabaseConnection, config: AppConfig) -> axum::Router {
    use randimg_core::handlers::{auth, author, crawler, health, image, pixiv_credential, statistic, tag, task};

    let queue_backend = db_backend::init(&config)
        .await
        .expect("Failed to init queue backend — is PostgreSQL running?");

    let state = Arc::new(randimg_core::WorkerState {
        db,
        config,
        oss: randimg_core::dogecloud::DogeCloudOss::new_noop(),
        queue_backend: queue_backend.clone(),
        http_client: reqwest::Client::new(),
        shutdown_token: tokio_util::sync::CancellationToken::new(),
        worker_start_time: std::time::Instant::now(),
        active_tasks: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        discover_cache: std::sync::Arc::new(dashmap::DashMap::new()),
        fingerprint_cache: queue_backend.fingerprint_cache.clone(),
        last_activity: std::sync::Arc::new(dashmap::DashMap::new()),
        stuck_pools: std::sync::Arc::new(dashmap::DashMap::new()),
    });

    axum::Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(tag::routes())
        .merge(author::routes())
        .merge(statistic::routes())
        .merge(image::routes())
        .merge(crawler::routes())
        .merge(task::routes())
        .merge(pixiv_credential::routes())
        .with_state(state)
}

/// Extract `(StatusCode, serde_json::Value)` from an Axum `Response`.
///
/// On parse failure the raw body text is stashed under the `"_raw"` key.
pub async fn response_json(resp: Response) -> (StatusCode, Value) {
    let status = resp.status();
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap_or_else(|_| {
        serde_json::json!({"_raw": String::from_utf8_lossy(&body_bytes).to_string()})
    });
    (status, json)
}

/// Create an admin user named `"admin"` with password `"pw"` and return
/// a valid JWT (`Bearer <token>`) for use in Authorization headers.
///
/// This is the single test admin convention — all auth-gated tests
/// should use this helper to avoid duplication.
pub async fn create_test_admin_and_token(
    db: &DatabaseConnection,
    config: &AppConfig,
) -> String {
    let pw_hash = password::hash_password("pw").unwrap();
    admin::create(db, "admin", &pw_hash, true)
        .await
        .expect("Failed to create test admin user");
    let token = jwt::create_token("admin", &config.secret_key, 60)
        .expect("Failed to create test JWT");
    format!("Bearer {}", token)
}

/// Build an `axum::http::Request` with a JSON body string.
///
/// Convenience for:
/// ```ignore
/// Request::builder()
///     .method("POST")
///     .uri("/endpoint")
///     .header("content-type", "application/json")
///     .body(Body::from(json_str))
/// ```
pub fn json_request(method: &str, uri: &str, json_body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(json_body.to_string()))
        .unwrap()
}

/// Build an `axum::http::Request` with a JSON body and an Authorization header.
pub fn json_request_with_auth(method: &str, uri: &str, json_body: &str, auth_header: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", auth_header)
        .body(Body::from(json_body.to_string()))
        .unwrap()
}

/// Build a GET `Request` with an Authorization header (no body).
pub fn get_request_with_auth(uri: &str, auth_header: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("authorization", auth_header)
        .body(Body::empty())
        .unwrap()
}

/// Build a DELETE `Request` with an Authorization header (no body).
pub fn delete_request_with_auth(uri: &str, auth_header: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .header("authorization", auth_header)
        .body(Body::empty())
        .unwrap()
}
