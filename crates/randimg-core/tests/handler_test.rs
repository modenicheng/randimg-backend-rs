//! Integration tests for Axum HTTP handlers.
//! Uses in-memory SQLite with real migrations — no external server needed.
//!
//! We build a minimal router (health + auth + tags + statistic + authors + image)
//! and test it via `tower::ServiceExt::oneshot`.

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

use randimg_core::config::{AppConfig, BindAddr};
use randimg_core::db_backend;

fn make_test_config() -> AppConfig {
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

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory SQLite");
    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");
    db
}

/// Build a router with a subset of handlers for testing.
async fn build_test_router(db: DatabaseConnection, config: AppConfig) -> axum::Router {
    use randimg_core::handlers::{health, auth, tag, author, statistic, image};

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
        .with_state(state)
}

async fn response_json(resp: Response) -> (StatusCode, Value) {
    let status = resp.status();
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap_or_else(|_| {
        serde_json::json!({"_raw": String::from_utf8_lossy(&body_bytes).to_string()})
    });
    (status, json)
}

// ── Health Endpoint ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_health_endpoint() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    assert_eq!(json["status"], "ok");
}

// ── Auth / Login Endpoint ────────────────────────────────────────────────────

#[tokio::test]
async fn test_login_success() {
    let db = setup_db().await;
    let config = make_test_config();

    let password_hash = randimg_core::auth::password::hash_password("testpassword").unwrap();
    randimg_core::db::query::admin::create(&db, "admin", &password_hash, true)
        .await
        .unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"admin","password":"testpassword"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    assert!(json["access_token"].is_string());
    assert_eq!(json["token_type"], "bearer");
}

#[tokio::test]
async fn test_login_wrong_password() {
    let db = setup_db().await;
    let config = make_test_config();

    let password_hash = randimg_core::auth::password::hash_password("correct").unwrap();
    randimg_core::db::query::admin::create(&db, "admin", &password_hash, true)
        .await
        .unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"admin","password":"wrong"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let (_, json) = response_json(resp).await;
    assert!(json["error"].is_string());
}

#[tokio::test]
async fn test_login_user_not_found() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"nobody","password":"pw"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Tags Endpoint ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_tags_empty() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder().uri("/tags").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    let tags = json.as_array().unwrap();
    assert!(tags.is_empty());
}

#[tokio::test]
async fn test_get_tags_with_data() {
    let db = setup_db().await;
    let config = make_test_config();

    randimg_core::db::query::tag::find_or_create(&db, "landscape", Some("风景")).await.unwrap();
    randimg_core::db::query::tag::find_or_create(&db, "portrait", Some("肖像")).await.unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder().uri("/tags").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    let tags = json.as_array().unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0]["name"], "landscape");
    assert_eq!(tags[0]["translated_name"], "风景");
    assert!(tags[0]["search_string"].is_string());
}

#[tokio::test]
async fn test_get_tags_with_pagination() {
    let db = setup_db().await;
    let config = make_test_config();

    for i in 0..5 {
        randimg_core::db::query::tag::find_or_create(&db, &format!("tag{}", i), None).await.unwrap();
    }

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder().uri("/tags?limit=2").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();

    let (_, json) = response_json(resp).await;
    let tags = json.as_array().unwrap();
    assert_eq!(tags.len(), 2);
}

#[tokio::test]
async fn test_update_tag_requires_auth() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/tags/1")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"translated_name":"test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_update_tag_with_auth() {
    let db = setup_db().await;
    let config = make_test_config();

    let tag = randimg_core::db::query::tag::find_or_create(&db, "sunset", None).await.unwrap();

    let pw_hash = randimg_core::auth::password::hash_password("pw").unwrap();
    randimg_core::db::query::admin::create(&db, "admin", &pw_hash, true).await.unwrap();
    let token = randimg_core::auth::jwt::create_token("admin", &config.secret_key, 60).unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/tags/{}", tag.id))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(r#"{"translated_name":"日落"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    assert_eq!(json["translated_name"], "日落");
}

#[tokio::test]
async fn test_update_tag_not_found() {
    let db = setup_db().await;
    let config = make_test_config();

    let pw_hash = randimg_core::auth::password::hash_password("pw").unwrap();
    randimg_core::db::query::admin::create(&db, "admin", &pw_hash, true).await.unwrap();
    let token = randimg_core::auth::jwt::create_token("admin", &config.secret_key, 60).unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/tags/99999")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(r#"{"translated_name":"nope"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_tag_with_auth() {
    let db = setup_db().await;
    let config = make_test_config();

    let tag = randimg_core::db::query::tag::find_or_create(&db, "deleteme", None).await.unwrap();

    let pw_hash = randimg_core::auth::password::hash_password("pw").unwrap();
    randimg_core::db::query::admin::create(&db, "admin", &pw_hash, true).await.unwrap();
    let token = randimg_core::auth::jwt::create_token("admin", &config.secret_key, 60).unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/tags/{}", tag.id))
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn test_delete_tag_not_found() {
    let db = setup_db().await;
    let config = make_test_config();

    let pw_hash = randimg_core::auth::password::hash_password("pw").unwrap();
    randimg_core::db::query::admin::create(&db, "admin", &pw_hash, true).await.unwrap();
    let token = randimg_core::auth::jwt::create_token("admin", &config.secret_key, 60).unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/tags/99999")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Authors Endpoint ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_authors_empty() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder().uri("/authors").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_list_authors_with_data() {
    let db = setup_db().await;
    let config = make_test_config();

    randimg_core::db::query::author::find_or_create(&db, "Artist1", Some("pixiv"), Some("p1")).await.unwrap();
    randimg_core::db::query::author::find_or_create(&db, "Artist2", Some("twitter"), Some("t1")).await.unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder().uri("/authors").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    let authors = json.as_array().unwrap();
    assert_eq!(authors.len(), 2);
}

#[tokio::test]
async fn test_get_author_by_id() {
    let db = setup_db().await;
    let config = make_test_config();

    let author = randimg_core::db::query::author::find_or_create(
        &db, "SoloArtist", Some("pixiv"), Some("solo1"),
    )
    .await
    .unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/authors/{}", author.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    assert_eq!(json["name"], "SoloArtist");
    assert!(json["images"].is_array());
}

#[tokio::test]
async fn test_get_author_not_found() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/authors/99999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Statistic Endpoint ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_statistic_empty_db() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder().uri("/statistic").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    assert_eq!(json["illust_count"], 0);
    assert_eq!(json["tag_count"], 0);
    assert_eq!(json["author_count"], 0);
}

#[tokio::test]
async fn test_statistic_with_data() {
    let db = setup_db().await;
    let config = make_test_config();

    randimg_core::db::query::tag::find_or_create(&db, "t1", None).await.unwrap();
    randimg_core::db::query::tag::find_or_create(&db, "t2", None).await.unwrap();
    randimg_core::db::query::author::find_or_create(&db, "A1", Some("p"), Some("1")).await.unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder().uri("/statistic").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    assert_eq!(json["illust_count"], 0);
    assert_eq!(json["tag_count"], 2);
    assert_eq!(json["author_count"], 1);
}

// ── Image Endpoints ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_random_image_not_found_when_empty() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder().uri("/").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_image_not_found() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder().uri("/image/99999").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_list_images_empty() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder().uri("/list").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = response_json(resp).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_list_images_invalid_sort_by() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/list?sort_by=invalid_column")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_color_search_missing_params() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/color/search")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_color_search_invalid_mode() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/color/search?r=128&g=128&b=128&mode=invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── PATCH /image/{id} Validation ─────────────────────────────────────────────

#[tokio::test]
async fn test_patch_image_requires_auth() {
    let db = setup_db().await;
    let config = make_test_config();
    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/image/1")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_patch_image_empty_title_rejected() {
    let db = setup_db().await;
    let config = make_test_config();

    let pw_hash = randimg_core::auth::password::hash_password("pw").unwrap();
    randimg_core::db::query::admin::create(&db, "admin", &pw_hash, true).await.unwrap();
    let token = randimg_core::auth::jwt::create_token("admin", &config.secret_key, 60).unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/image/1")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(r#"{"title":""}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_patch_image_invalid_accessible_type() {
    let db = setup_db().await;
    let config = make_test_config();

    let pw_hash = randimg_core::auth::password::hash_password("pw").unwrap();
    randimg_core::db::query::admin::create(&db, "admin", &pw_hash, true).await.unwrap();
    let token = randimg_core::auth::jwt::create_token("admin", &config.secret_key, 60).unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/image/1")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(r#"{"accessible":"not_bool"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_patch_image_invalid_colors_type() {
    let db = setup_db().await;
    let config = make_test_config();

    let pw_hash = randimg_core::auth::password::hash_password("pw").unwrap();
    randimg_core::db::query::admin::create(&db, "admin", &pw_hash, true).await.unwrap();
    let token = randimg_core::auth::jwt::create_token("admin", &config.secret_key, 60).unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/image/1")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(r#"{"colors":"invalid"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_patch_image_not_found() {
    let db = setup_db().await;
    let config = make_test_config();

    let pw_hash = randimg_core::auth::password::hash_password("pw").unwrap();
    randimg_core::db::query::admin::create(&db, "admin", &pw_hash, true).await.unwrap();
    let token = randimg_core::auth::jwt::create_token("admin", &config.secret_key, 60).unwrap();

    let app = build_test_router(db, config).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/image/99999")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(r#"{"title":"new title"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
