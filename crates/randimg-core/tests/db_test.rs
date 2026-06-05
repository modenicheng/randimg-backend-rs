//! Integration tests for the database query layer.
//! Uses in-memory SQLite with real migrations — no external DB needed.

use migration::MigratorTrait;
use sea_orm::{Database, DatabaseConnection};

/// Create an in-memory SQLite connection and run all migrations.
async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory SQLite");
    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");
    db
}

// ── Admin Query Tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_admin_create_and_find_by_username() {
    let db = setup_db().await;
    let admin = randimg_core::db::query::admin::create(&db, "testuser", "hashed_pw", true)
        .await
        .unwrap();
    assert_eq!(admin.username, "testuser");
    assert_eq!(admin.password, "hashed_pw");
    assert!(admin.is_superuser);

    let found = randimg_core::db::query::admin::find_by_username(&db, "testuser")
        .await
        .unwrap();
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.id, admin.id);
    assert_eq!(found.username, "testuser");
}

#[tokio::test]
async fn test_admin_find_by_username_not_found() {
    let db = setup_db().await;
    let found = randimg_core::db::query::admin::find_by_username(&db, "nonexistent")
        .await
        .unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_admin_create_non_superuser() {
    let db = setup_db().await;
    let admin = randimg_core::db::query::admin::create(&db, "regular", "pw", false)
        .await
        .unwrap();
    assert!(!admin.is_superuser);
}

// ── Tag Query Tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_tag_find_or_create_new() {
    let db = setup_db().await;
    let tag = randimg_core::db::query::tag::find_or_create(&db, "landscape", Some("风景"))
        .await
        .unwrap();
    assert_eq!(tag.name, "landscape");
    assert_eq!(tag.translated_name.as_deref(), Some("风景"));
}

#[tokio::test]
async fn test_tag_find_or_create_existing() {
    let db = setup_db().await;
    let tag1 = randimg_core::db::query::tag::find_or_create(&db, "portrait", Some("肖像"))
        .await
        .unwrap();
    let tag2 = randimg_core::db::query::tag::find_or_create(&db, "portrait", Some("different"))
        .await
        .unwrap();
    // Should return the existing tag, not create a new one
    assert_eq!(tag1.id, tag2.id);
    assert_eq!(tag2.translated_name.as_deref(), Some("肖像")); // original value preserved
}

#[tokio::test]
async fn test_tag_find_all() {
    let db = setup_db().await;
    randimg_core::db::query::tag::find_or_create(&db, "a", None).await.unwrap();
    randimg_core::db::query::tag::find_or_create(&db, "b", None).await.unwrap();
    randimg_core::db::query::tag::find_or_create(&db, "c", None).await.unwrap();

    let all = randimg_core::db::query::tag::find_all(&db, None, None).await.unwrap();
    assert_eq!(all.len(), 3);

    let limited = randimg_core::db::query::tag::find_all(&db, Some(2), None).await.unwrap();
    assert_eq!(limited.len(), 2);

    // SQLite requires LIMIT when using OFFSET
    let offset = randimg_core::db::query::tag::find_all(&db, Some(10), Some(1)).await.unwrap();
    assert_eq!(offset.len(), 2);
}

#[tokio::test]
async fn test_tag_update_tag() {
    let db = setup_db().await;
    let tag = randimg_core::db::query::tag::find_or_create(&db, "sunset", None)
        .await
        .unwrap();

    let updated = randimg_core::db::query::tag::update_tag(&db, tag.id, Some("日落"))
        .await
        .unwrap();
    assert!(updated.is_some());
    let updated = updated.unwrap();
    assert_eq!(updated.translated_name.as_deref(), Some("日落"));
}

#[tokio::test]
async fn test_tag_update_not_found() {
    let db = setup_db().await;
    let result = randimg_core::db::query::tag::update_tag(&db, 9999, Some("test"))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_tag_find_or_create_no_translated_name() {
    let db = setup_db().await;
    let tag = randimg_core::db::query::tag::find_or_create(&db, "abstract", None)
        .await
        .unwrap();
    assert!(tag.translated_name.is_none());
}

// ── Author Query Tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_author_find_or_create_new() {
    let db = setup_db().await;
    let author = randimg_core::db::query::author::find_or_create(
        &db, "TestArtist", Some("pixiv"), Some("12345"),
    )
    .await
    .unwrap();
    assert_eq!(author.name, "TestArtist");
    assert_eq!(author.platform.as_deref(), Some("pixiv"));
    assert_eq!(author.platform_id.as_deref(), Some("12345"));
}

#[tokio::test]
async fn test_author_find_or_create_by_platform_id() {
    let db = setup_db().await;
    let a1 = randimg_core::db::query::author::find_or_create(
        &db, "Artist1", Some("pixiv"), Some("pid001"),
    )
    .await
    .unwrap();
    // Same platform_id, different name → returns existing
    let a2 = randimg_core::db::query::author::find_or_create(
        &db, "Artist2", Some("pixiv"), Some("pid001"),
    )
    .await
    .unwrap();
    assert_eq!(a1.id, a2.id);
    assert_eq!(a2.name, "Artist1"); // original name preserved
}

#[tokio::test]
async fn test_author_find_or_create_by_name() {
    let db = setup_db().await;
    let a1 = randimg_core::db::query::author::find_or_create(
        &db, "SameName", Some("pixiv"), None,
    )
    .await
    .unwrap();
    let a2 = randimg_core::db::query::author::find_or_create(
        &db, "SameName", Some("twitter"), None,
    )
    .await
    .unwrap();
    assert_eq!(a1.id, a2.id);
}

#[tokio::test]
async fn test_author_count() {
    let db = setup_db().await;
    assert_eq!(randimg_core::db::query::author::count(&db).await.unwrap(), 0);

    randimg_core::db::query::author::find_or_create(&db, "A1", Some("p"), Some("1")).await.unwrap();
    randimg_core::db::query::author::find_or_create(&db, "A2", Some("p"), Some("2")).await.unwrap();
    assert_eq!(randimg_core::db::query::author::count(&db).await.unwrap(), 2);
}

#[tokio::test]
async fn test_author_find_all_with_pagination() {
    let db = setup_db().await;
    for i in 0..5 {
        randimg_core::db::query::author::find_or_create(
            &db, &format!("Author{}", i), Some("p"), Some(&format!("{}", i)),
        )
        .await
        .unwrap();
    }

    let all = randimg_core::db::query::author::find_all(&db, None, None).await.unwrap();
    assert_eq!(all.len(), 5);

    let page = randimg_core::db::query::author::find_all(&db, Some(2), Some(1)).await.unwrap();
    assert_eq!(page.len(), 2);
}

#[tokio::test]
async fn test_author_find_by_id() {
    let db = setup_db().await;
    let author = randimg_core::db::query::author::find_or_create(
        &db, "FindMe", Some("p"), Some("fid"),
    )
    .await
    .unwrap();

    let found = randimg_core::db::query::author::find_by_id(&db, author.id).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "FindMe");

    let not_found = randimg_core::db::query::author::find_by_id(&db, 99999).await.unwrap();
    assert!(not_found.is_none());
}

// ── Crawler Query Tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_crawler_create_and_find() {
    let db = setup_db().await;
    let crawler = randimg_core::db::query::crawler::create(
        &db, "test crawl", 1, Some("user123"), None, None, None,
    )
    .await
    .unwrap();
    assert_eq!(crawler.task_name, "test crawl");
    assert_eq!(crawler.crawl_type, 1);
    assert_eq!(crawler.status, 0);
    assert_eq!(crawler.target_user_id.as_deref(), Some("user123"));

    let found = randimg_core::db::query::crawler::find_by_id(&db, crawler.id)
        .await
        .unwrap();
    assert!(found.is_some());
}

#[tokio::test]
async fn test_crawler_find_all() {
    let db = setup_db().await;
    randimg_core::db::query::crawler::create(&db, "c1", 0, None, None, None, None).await.unwrap();
    randimg_core::db::query::crawler::create(&db, "c2", 1, Some("u"), None, None, None).await.unwrap();

    let all = randimg_core::db::query::crawler::find_all(&db).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn test_crawler_mark_running() {
    let db = setup_db().await;
    let c = randimg_core::db::query::crawler::create(&db, "run", 1, Some("u"), None, None, None).await.unwrap();
    assert_eq!(c.status, 0);
    assert!(c.start_time.is_none());

    randimg_core::db::query::crawler::mark_running(&db, c.id).await.unwrap();
    let updated = randimg_core::db::query::crawler::find_by_id(&db, c.id).await.unwrap().unwrap();
    assert_eq!(updated.status, 1);
    assert!(updated.start_time.is_some());
}

#[tokio::test]
async fn test_crawler_mark_completed() {
    let db = setup_db().await;
    let c = randimg_core::db::query::crawler::create(&db, "comp", 1, Some("u"), None, None, None).await.unwrap();

    randimg_core::db::query::crawler::mark_completed(&db, c.id, 10).await.unwrap();
    let updated = randimg_core::db::query::crawler::find_by_id(&db, c.id).await.unwrap().unwrap();
    assert_eq!(updated.status, 2);
    assert!(updated.end_time.is_some());
    assert_eq!(updated.total_pages, Some(10));
    assert_eq!(updated.processed_pages, Some(10));
}

#[tokio::test]
async fn test_crawler_mark_failed() {
    let db = setup_db().await;
    let c = randimg_core::db::query::crawler::create(&db, "fail", 1, Some("u"), None, None, None).await.unwrap();

    randimg_core::db::query::crawler::mark_failed(&db, c.id).await.unwrap();
    let updated = randimg_core::db::query::crawler::find_by_id(&db, c.id).await.unwrap().unwrap();
    assert_eq!(updated.status, 99);
    assert!(updated.end_time.is_some());
}

#[tokio::test]
async fn test_crawler_find_by_id_not_found() {
    let db = setup_db().await;
    let result = randimg_core::db::query::crawler::find_by_id(&db, 99999).await.unwrap();
    assert!(result.is_none());
}

// ── PixivCredential Query Tests ──────────────────────────────────────────────

#[tokio::test]
async fn test_pixiv_credential_create_and_find() {
    let db = setup_db().await;
    let cred = randimg_core::db::query::pixiv_credential::create(
        &db, "user1", "refresh_token_abc", Some("test note"),
    )
    .await
    .unwrap();
    assert_eq!(cred.pixiv_user_id, "user1");
    assert_eq!(cred.refresh_token, "refresh_token_abc");
    assert_eq!(cred.status, 0); // STATUS_ACTIVE
    assert_eq!(cred.note.as_deref(), Some("test note"));

    let found = randimg_core::db::query::pixiv_credential::find_by_id(&db, cred.id)
        .await
        .unwrap();
    assert!(found.is_some());
}

#[tokio::test]
async fn test_pixiv_credential_find_all() {
    let db = setup_db().await;
    randimg_core::db::query::pixiv_credential::create(&db, "u1", "rt1", None).await.unwrap();
    randimg_core::db::query::pixiv_credential::create(&db, "u2", "rt2", None).await.unwrap();

    let all = randimg_core::db::query::pixiv_credential::find_all(&db).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn test_pixiv_credential_update_partial() {
    let db = setup_db().await;
    let cred = randimg_core::db::query::pixiv_credential::create(&db, "u1", "rt", None).await.unwrap();

    // Update only status
    let updated = randimg_core::db::query::pixiv_credential::update(
        &db, cred.id, None, None, Some(1), None,
    )
    .await
    .unwrap();
    assert!(updated.is_some());
    let updated = updated.unwrap();
    assert_eq!(updated.status, 1);
    assert_eq!(updated.refresh_token, "rt"); // unchanged
}

#[tokio::test]
async fn test_pixiv_credential_update_token() {
    let db = setup_db().await;
    let cred = randimg_core::db::query::pixiv_credential::create(&db, "u1", "old_rt", None).await.unwrap();

    randimg_core::db::query::pixiv_credential::update_token(&db, cred.id, "new_rt", Some("new_at"), None).await.unwrap();

    let updated = randimg_core::db::query::pixiv_credential::find_by_id(&db, cred.id).await.unwrap().unwrap();
    assert_eq!(updated.refresh_token, "new_rt");
    assert_eq!(updated.access_token.as_deref(), Some("new_at"));
    assert!(updated.last_refreshed_at.is_some());
}

#[tokio::test]
async fn test_pixiv_credential_update_status() {
    let db = setup_db().await;
    let cred = randimg_core::db::query::pixiv_credential::create(&db, "u1", "rt", None).await.unwrap();

    randimg_core::db::query::pixiv_credential::update_status(&db, cred.id, 2).await.unwrap();
    let updated = randimg_core::db::query::pixiv_credential::find_by_id(&db, cred.id).await.unwrap().unwrap();
    assert_eq!(updated.status, 2);
}

#[tokio::test]
async fn test_pixiv_credential_touch_last_used() {
    let db = setup_db().await;
    let cred = randimg_core::db::query::pixiv_credential::create(&db, "u1", "rt", None).await.unwrap();
    assert!(cred.last_used_at.is_none());

    randimg_core::db::query::pixiv_credential::touch_last_used(&db, cred.id).await.unwrap();
    let updated = randimg_core::db::query::pixiv_credential::find_by_id(&db, cred.id).await.unwrap().unwrap();
    assert!(updated.last_used_at.is_some());
}

#[tokio::test]
async fn test_pixiv_credential_delete() {
    let db = setup_db().await;
    let cred = randimg_core::db::query::pixiv_credential::create(&db, "u1", "rt", None).await.unwrap();

    let deleted = randimg_core::db::query::pixiv_credential::delete(&db, cred.id).await.unwrap();
    assert!(deleted);

    let found = randimg_core::db::query::pixiv_credential::find_by_id(&db, cred.id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_pixiv_credential_delete_not_found() {
    let db = setup_db().await;
    let deleted = randimg_core::db::query::pixiv_credential::delete(&db, 99999).await.unwrap();
    assert!(!deleted);
}

#[tokio::test]
async fn test_pixiv_credential_find_one_active_random_none_when_empty() {
    let db = setup_db().await;
    let result = randimg_core::db::query::pixiv_credential::find_one_active_random(&db).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_pixiv_credential_find_one_active_random_returns_active() {
    let db = setup_db().await;
    randimg_core::db::query::pixiv_credential::create(&db, "u1", "rt1", None).await.unwrap();
    // Create an expired one
    let expired = randimg_core::db::query::pixiv_credential::create(&db, "u2", "rt2", None).await.unwrap();
    randimg_core::db::query::pixiv_credential::update_status(&db, expired.id, 1).await.unwrap();

    let result = randimg_core::db::query::pixiv_credential::find_one_active_random(&db).await.unwrap();
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.pixiv_user_id, "u1"); // only active one
}

#[tokio::test]
async fn test_pixiv_credential_update_not_found() {
    let db = setup_db().await;
    let result = randimg_core::db::query::pixiv_credential::update(
        &db, 99999, Some("rt"), None, None, None,
    )
    .await
    .unwrap();
    assert!(result.is_none());
}

// ── Image Query Tests ────────────────────────────────────────────────────────

fn make_test_config() -> randimg_core::config::AppConfig {
    randimg_core::config::AppConfig {
        api_database_url: "postgres://localhost/test_db".into(),
        queue_database_url: "postgres://localhost/test_queue".into(),
        secret_key: "test-secret".into(),
        jwt_expire_minutes: 60,
        cdn_base_url: "https://cdn.test/".into(),
        image_dir: "/tmp/test_images".into(),
        server_addr: randimg_core::config::BindAddr::parse("127.0.0.1:8080"),
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

async fn create_test_image(db: &sea_orm::DatabaseConnection, author_id: i32, title: &str) -> randimg_core::db::entities::image::Model {
    let data = serde_json::json!({
        "title": title,
        "image_path": format!("{}.jpg", title),
        "source_url": format!("https://pixiv.net/{}", title),
        "source_id": 1000,
        "author_id": author_id,
        "width": 1920,
        "height": 1080,
        "aspect_ratio": 1.78,
        "total_view": 1000,
        "total_bookmarks": 50,
        "total_comments": 10
    });
    randimg_core::db::query::image::create_image(db, &data).await.unwrap()
}

#[tokio::test]
async fn test_image_create_and_find_by_id() {
    let db = setup_db().await;
    let config = make_test_config();
    let author = randimg_core::db::query::author::find_or_create(&db, "Artist", Some("p"), Some("1")).await.unwrap();
    let img = create_test_image(&db, author.id, "test_img").await;

    // Non-admin, non-public image should not be visible (accessible check)
    // First make it public
    let update_data = serde_json::json!({"is_public": true, "accessible": true});
    randimg_core::db::query::image::update_fields(&db, img.id, update_data).await.unwrap();

    let found = randimg_core::db::query::image::find_by_id(&db, img.id, false, &config).await.unwrap();
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found["title"], "test_img");
    assert_eq!(found["width"], 1920);
}

#[tokio::test]
async fn test_image_find_by_id_not_found() {
    let db = setup_db().await;
    let config = make_test_config();
    let found = randimg_core::db::query::image::find_by_id(&db, 99999, false, &config).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_image_find_by_id_inaccessible_hidden_for_non_admin() {
    let db = setup_db().await;
    let config = make_test_config();
    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();
    let img = create_test_image(&db, author.id, "hidden").await;

    // Mark as accessible=false
    let update_data = serde_json::json!({"accessible": false, "is_public": true});
    randimg_core::db::query::image::update_fields(&db, img.id, update_data).await.unwrap();

    // Non-admin should not see it
    let found = randimg_core::db::query::image::find_by_id(&db, img.id, false, &config).await.unwrap();
    assert!(found.is_none());

    // Admin should see it
    let found = randimg_core::db::query::image::find_by_id(&db, img.id, true, &config).await.unwrap();
    assert!(found.is_some());
}

#[tokio::test]
async fn test_image_update_fields() {
    let db = setup_db().await;
    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();
    let img = create_test_image(&db, author.id, "updatable").await;

    let data = serde_json::json!({
        "title": "Updated Title",
        "is_public": true,
        "accessible": true
    });
    let updated = randimg_core::db::query::image::update_fields(&db, img.id, data).await.unwrap();
    assert!(updated.is_some());
    let updated = updated.unwrap();
    assert_eq!(updated.title, "Updated Title");
    assert!(updated.is_public);
    assert_eq!(updated.accessible, Some(true));
}

#[tokio::test]
async fn test_image_update_fields_not_found() {
    let db = setup_db().await;
    let result = randimg_core::db::query::image::update_fields(
        &db, 99999, serde_json::json!({"title": "nope"}),
    )
    .await
    .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_image_find_unprocessed() {
    let db = setup_db().await;
    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();

    // Create an unprocessed image (is_public=false, primary_l=null)
    create_test_image(&db, author.id, "unprocessed").await;

    // Create a processed image
    let processed = create_test_image(&db, author.id, "processed").await;
    let update_data = serde_json::json!({"is_public": true});
    randimg_core::db::query::image::update_fields(&db, processed.id, update_data).await.unwrap();

    let unprocessed = randimg_core::db::query::image::find_unprocessed(&db).await.unwrap();
    assert_eq!(unprocessed.len(), 1);
    assert_eq!(unprocessed[0].title, "unprocessed");
}

#[tokio::test]
async fn test_image_count_accessible() {
    let db = setup_db().await;
    assert_eq!(randimg_core::db::query::image::count_accessible(&db).await.unwrap(), 0);

    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();
    let img1 = create_test_image(&db, author.id, "img1").await;
    let img2 = create_test_image(&db, author.id, "img2").await;

    // Mark one as accessible
    randimg_core::db::query::image::update_fields(&db, img1.id, serde_json::json!({"accessible": true})).await.unwrap();
    assert_eq!(randimg_core::db::query::image::count_accessible(&db).await.unwrap(), 1);

    // Mark both as accessible
    randimg_core::db::query::image::update_fields(&db, img2.id, serde_json::json!({"accessible": true})).await.unwrap();
    assert_eq!(randimg_core::db::query::image::count_accessible(&db).await.unwrap(), 2);
}

#[tokio::test]
async fn test_image_list_images() {
    let db = setup_db().await;
    let config = make_test_config();
    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();

    for i in 0..5 {
        let img = create_test_image(&db, author.id, &format!("img{}", i)).await;
        randimg_core::db::query::image::update_fields(&db, img.id, serde_json::json!({"is_public": true, "accessible": true})).await.unwrap();
    }

    let results = randimg_core::db::query::image::list_images(
        &db, 0, 10, true, "id", 0.0, 10.0, 0, i32::MAX, 0, i32::MAX,
        None, None, None, false, &config,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 5);

    // Paginated
    let page = randimg_core::db::query::image::list_images(
        &db, 0, 2, true, "id", 0.0, 10.0, 0, i32::MAX, 0, i32::MAX,
        None, None, None, false, &config,
    )
    .await
    .unwrap();
    assert_eq!(page.len(), 2);
}

#[tokio::test]
async fn test_image_list_images_sorted_by_width() {
    let db = setup_db().await;
    let config = make_test_config();
    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();

    // Create images with different widths
    for (i, width) in [(100, 800), (200, 1200), (300, 640)].iter() {
        let data = serde_json::json!({
            "title": format!("img{}", i),
            "image_path": format!("img{}.jpg", i),
            "author_id": author.id,
            "width": width,
            "height": 600,
            "aspect_ratio": *width as f32 / 600.0,
        });
        let img = randimg_core::db::query::image::create_image(&db, &data).await.unwrap();
        randimg_core::db::query::image::update_fields(&db, img.id, serde_json::json!({"is_public": true, "accessible": true})).await.unwrap();
    }

    let results = randimg_core::db::query::image::list_images(
        &db, 0, 10, false, "width", 0.0, 10.0, 0, i32::MAX, 0, i32::MAX,
        None, None, None, false, &config,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 3);
    // Should be sorted ascending by width: 640, 800, 1200
    assert_eq!(results[0]["width"], 640);
    assert_eq!(results[1]["width"], 800);
    assert_eq!(results[2]["width"], 1200);
}

#[tokio::test]
async fn test_image_list_images_by_author_filter() {
    let db = setup_db().await;
    let config = make_test_config();
    let a1 = randimg_core::db::query::author::find_or_create(&db, "Alice", Some("p"), Some("1")).await.unwrap();
    let a2 = randimg_core::db::query::author::find_or_create(&db, "Bob", Some("p"), Some("2")).await.unwrap();

    let img1 = create_test_image(&db, a1.id, "alice_img").await;
    let img2 = create_test_image(&db, a2.id, "bob_img").await;
    randimg_core::db::query::image::update_fields(&db, img1.id, serde_json::json!({"is_public": true, "accessible": true})).await.unwrap();
    randimg_core::db::query::image::update_fields(&db, img2.id, serde_json::json!({"is_public": true, "accessible": true})).await.unwrap();

    // Filter by author name
    let results = randimg_core::db::query::image::list_images(
        &db, 0, 10, true, "id", 0.0, 10.0, 0, i32::MAX, 0, i32::MAX,
        Some("Alice"), None, None, false, &config,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["title"], "alice_img");
}

#[tokio::test]
async fn test_image_color_search_primary() {
    let db = setup_db().await;
    let config = make_test_config();
    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();

    let img = create_test_image(&db, author.id, "colorful").await;
    randimg_core::db::query::image::update_fields(
        &db, img.id,
        serde_json::json!({
            "is_public": true,
            "accessible": true,
            "colors": {"primary_color": [255, 0, 0]}
        }),
    ).await.unwrap();

    // Update primary LAB values directly via raw update
    use sea_orm::{EntityTrait, ActiveModelTrait, Set};
    use randimg_core::db::entities::image::{Entity as ImageEntity, ActiveModel};
    let existing = ImageEntity::find_by_id(img.id).one(&db).await.unwrap().unwrap();
    let mut active: ActiveModel = existing.into();
    active.primary_l = Set(Some(53.23));
    active.primary_a = Set(Some(80.11));
    active.primary_b = Set(Some(67.22));
    active.update(&db).await.unwrap();

    // Search by LAB (close to red)
    let results = randimg_core::db::query::image::color_search(
        &db, [53.0, 80.0, 67.0], "primary", Some(100.0), 10, &config,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["id"], img.id);
}

#[tokio::test]
async fn test_image_color_search_no_results() {
    let db = setup_db().await;
    let config = make_test_config();

    let results = randimg_core::db::query::image::color_search(
        &db, [50.0, 0.0, 0.0], "primary", Some(1.0), 10, &config,
    )
    .await
    .unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_image_create_with_source_created_at() {
    let db = setup_db().await;
    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();

    let data = serde_json::json!({
        "title": "dated",
        "image_path": "dated.jpg",
        "author_id": author.id,
        "width": 800,
        "height": 600,
        "aspect_ratio": 1.33,
        "source_created_at": "2026-01-15 10:30:00"
    });
    let img = randimg_core::db::query::image::create_image(&db, &data).await.unwrap();
    assert!(img.source_created_at.is_some());
}

#[tokio::test]
async fn test_seed_method_from_str() {
    use randimg_core::db::query::image::SeedMethod;
    assert!(matches!(SeedMethod::from_str("views"), SeedMethod::Views));
    assert!(matches!(SeedMethod::from_str("bookmarks"), SeedMethod::Bookmarks));
    assert!(matches!(SeedMethod::from_str("random"), SeedMethod::Random));
    assert!(matches!(SeedMethod::from_str("popularity"), SeedMethod::Popularity));
    assert!(matches!(SeedMethod::from_str("unknown"), SeedMethod::Popularity));
    assert!(matches!(SeedMethod::from_str(""), SeedMethod::Popularity));
}

// ── Image Tag Association Tests ──────────────────────────────────────────────

#[tokio::test]
async fn test_image_with_tags() {
    let db = setup_db().await;
    let config = make_test_config();
    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();
    let img = create_test_image(&db, author.id, "tagged").await;

    // Make image public
    randimg_core::db::query::image::update_fields(&db, img.id, serde_json::json!({"is_public": true, "accessible": true})).await.unwrap();

    // Add tags via association table
    let tag1 = randimg_core::db::query::tag::find_or_create(&db, "nature", None).await.unwrap();
    let tag2 = randimg_core::db::query::tag::find_or_create(&db, "sunset", Some("日落")).await.unwrap();

    use sea_orm::ActiveModelTrait;
    use randimg_core::db::entities::image_tag_association::{ActiveModel as AssocActiveModel};
    AssocActiveModel {
        image_id: sea_orm::Set(img.id),
        tag_id: sea_orm::Set(tag1.id),
    }.insert(&db).await.unwrap();
    AssocActiveModel {
        image_id: sea_orm::Set(img.id),
        tag_id: sea_orm::Set(tag2.id),
    }.insert(&db).await.unwrap();

    let found = randimg_core::db::query::image::find_by_id(&db, img.id, false, &config).await.unwrap().unwrap();
    let tags = found["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 2);
}

#[tokio::test]
async fn test_image_list_with_tag_filter() {
    let db = setup_db().await;
    let config = make_test_config();
    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();

    let img1 = create_test_image(&db, author.id, "nature_img").await;
    let img2 = create_test_image(&db, author.id, "city_img").await;
    randimg_core::db::query::image::update_fields(&db, img1.id, serde_json::json!({"is_public": true, "accessible": true})).await.unwrap();
    randimg_core::db::query::image::update_fields(&db, img2.id, serde_json::json!({"is_public": true, "accessible": true})).await.unwrap();

    let nature_tag = randimg_core::db::query::tag::find_or_create(&db, "nature", None).await.unwrap();

    use sea_orm::ActiveModelTrait;
    use randimg_core::db::entities::image_tag_association::ActiveModel as AssocActiveModel;
    AssocActiveModel {
        image_id: sea_orm::Set(img1.id),
        tag_id: sea_orm::Set(nature_tag.id),
    }.insert(&db).await.unwrap();

    let results = randimg_core::db::query::image::list_images(
        &db, 0, 10, true, "id", 0.0, 10.0, 0, i32::MAX, 0, i32::MAX,
        None, None, Some("nature"), false, &config,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["title"], "nature_img");
}

// ── Soft Delete Tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_soft_deleted_image_excluded_from_list() {
    let db = setup_db().await;
    let config = make_test_config();
    let author = randimg_core::db::query::author::find_or_create(&db, "A", Some("p"), Some("1")).await.unwrap();

    let img = create_test_image(&db, author.id, "to_delete").await;
    randimg_core::db::query::image::update_fields(&db, img.id, serde_json::json!({"is_public": true, "accessible": true})).await.unwrap();

    // Verify it appears
    let results = randimg_core::db::query::image::list_images(
        &db, 0, 10, true, "id", 0.0, 10.0, 0, i32::MAX, 0, i32::MAX,
        None, None, None, false, &config,
    ).await.unwrap();
    assert_eq!(results.len(), 1);

    // Soft delete
    use sea_orm::{EntityTrait, ActiveModelTrait, Set};
    use randimg_core::db::entities::image::Entity as ImageEntity;
    let existing = ImageEntity::find_by_id(img.id).one(&db).await.unwrap().unwrap();
    let mut active: randimg_core::db::entities::image::ActiveModel = existing.into();
    active.deleted_at = Set(Some(chrono::Utc::now().fixed_offset()));
    active.is_public = Set(false);
    active.update(&db).await.unwrap();

    // Should be excluded
    let results = randimg_core::db::query::image::list_images(
        &db, 0, 10, true, "id", 0.0, 10.0, 0, i32::MAX, 0, i32::MAX,
        None, None, None, false, &config,
    ).await.unwrap();
    assert_eq!(results.len(), 0);
}

// ── Task Increment Retry Tests ────────────────────────────────────────────────

#[tokio::test]
async fn test_task_increment_retry_from_zero() {
    let db = setup_db().await;
    let task = randimg_core::db::query::task::create(
        &db, "crawl", None, None, None, None, Some("{}"),
    )
    .await
    .unwrap();
    assert_eq!(task.retry_count, 0);

    randimg_core::db::query::task::increment_retry(&db, &task.id)
        .await
        .unwrap();

    let updated = randimg_core::db::query::task::find_by_id(&db, &task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.retry_count, 1);
}

#[tokio::test]
async fn test_task_increment_retry_idempotent() {
    let db = setup_db().await;
    let task = randimg_core::db::query::task::create(
        &db, "crawl", None, None, None, None, Some("{}"),
    )
    .await
    .unwrap();

    for expected in 1..=3 {
        randimg_core::db::query::task::increment_retry(&db, &task.id)
            .await
            .unwrap();
        let updated = randimg_core::db::query::task::find_by_id(&db, &task.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.retry_count, expected);
    }
}

#[tokio::test]
async fn test_task_increment_retry_not_found_is_noop() {
    let db = setup_db().await;
    // Should not error when task doesn't exist
    let result = randimg_core::db::query::task::increment_retry(&db, "nonexistent-id").await;
    assert!(result.is_ok());
}

// ── Derived status flag tests (pure function, no DB needed) ─────────────────

/// "Active" must short-circuit the rollup. A root with both active and failed
/// descendants is still "running" — the user shouldn't see a transient failure
/// surface as "failed" while other descendants are still in flight.
#[test]
fn derived_status_active_overrides_failed() {
    use randimg_core::db::query::task_tree::derived_status_from_flags;
    assert_eq!(derived_status_from_flags(true,  true,  false, true),  "running");
    assert_eq!(derived_status_from_flags(true,  true,  true,  true),  "running");
    assert_eq!(derived_status_from_flags(true,  false, true,  true),  "running");
    assert_eq!(derived_status_from_flags(true,  false, false, false), "running");
}

/// Once every descendant has settled, surface the terminal outcome.
#[test]
fn derived_status_terminal_outcomes() {
    use randimg_core::db::query::task_tree::derived_status_from_flags;
    // failed + completed → partial_success (mixed)
    assert_eq!(derived_status_from_flags(false, true,  true,  true),  "partial_success");
    // failed only, transient
    assert_eq!(derived_status_from_flags(false, true,  false, false), "failed");
    // completed only
    assert_eq!(derived_status_from_flags(false, false, true,  false), "completed");
    // nothing happened (rare: root Done with no descendants should not reach rollup,
    // but if it does we degrade to "pending" rather than crashing)
    assert_eq!(derived_status_from_flags(false, false, false, false), "pending");
}

/// When every failed descendant has reached the terminal `Killed` state and
/// nothing succeeded, the subtree is dead — surface `killed`, not `failed` (the
/// latter would imply retries are still possible).
#[test]
fn derived_status_all_killed_is_terminal_killed() {
    use randimg_core::db::query::task_tree::derived_status_from_flags;
    // has_failed == has_killed_terminal: every failure is terminal
    assert_eq!(derived_status_from_flags(false, true, false, true), "killed");
    // Mixed: some still transient `Failed` (has_killed_terminal < has_failed) →
    // still "failed" (the user might be able to salvage something).
    assert_eq!(derived_status_from_flags(false, true, false, false), "failed");
}
