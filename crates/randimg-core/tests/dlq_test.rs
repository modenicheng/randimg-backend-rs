use migration::MigratorTrait;
use sea_orm::{Database, DatabaseConnection};
use serde_json::json;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory SQLite");
    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");
    db
}

#[tokio::test]
async fn test_dead_letter_insert_and_list() {
    let db = setup_db().await;

    let task = randimg_core::db::query::task::create(
        &db, "crawl", None, None, None, None, Some(r#"{"target":"test"}"#),
    )
    .await
    .unwrap();

    let history = json!([
        { "attempt": 1, "error": "timeout", "timestamp": "2026-01-01T00:00:00Z" },
        { "attempt": 2, "error": "connection reset", "timestamp": "2026-01-01T00:01:00Z" }
    ]);

    let dl = randimg_core::db::query::dead_letter::insert_dead_letter(
        &db,
        &task.id,
        "crawl",
        Some(r#"{"target":"test"}"#),
        "connection reset",
        3,
        Some(history),
    )
    .await
    .unwrap();

    assert_eq!(dl.task_id, task.id);
    assert_eq!(dl.task_type, "crawl");
    assert_eq!(dl.params.as_deref(), Some(r#"{"target":"test"}"#));
    assert_eq!(dl.error_message, "connection reset");
    assert_eq!(dl.retry_count, 3);
    assert!(dl.failure_history.is_some());

    let history_val = dl.failure_history.as_ref().unwrap();
    assert_eq!(history_val.as_array().unwrap().len(), 2);

    let listed = randimg_core::db::query::dead_letter::list_dead_letters(&db, None, 10, 0)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, dl.id);

    let filtered = randimg_core::db::query::dead_letter::list_dead_letters(&db, Some("download"), 10, 0)
        .await
        .unwrap();
    assert_eq!(filtered.len(), 0);

    let by_type = randimg_core::db::query::dead_letter::list_dead_letters(&db, Some("crawl"), 10, 0)
        .await
        .unwrap();
    assert_eq!(by_type.len(), 1);

    let found = randimg_core::db::query::dead_letter::get_dead_letter(&db, &dl.id)
        .await
        .unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().task_id, task.id);

    let not_found = randimg_core::db::query::dead_letter::get_dead_letter(&db, "nonexistent")
        .await
        .unwrap();
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_dead_letter_requeue() {
    let db = setup_db().await;

    let task = randimg_core::db::query::task::create(
        &db, "download", None, None, None, None, Some(r#"{"image_id":42}"#),
    )
    .await
    .unwrap();

    let dl = randimg_core::db::query::dead_letter::insert_dead_letter(
        &db,
        &task.id,
        "download",
        Some(r#"{"image_id":42}"#),
        "permanent failure",
        3,
        None,
    )
    .await
    .unwrap();

    let new_task = randimg_core::db::query::dead_letter::requeue_dead_letter(&db, &dl.id)
        .await
        .unwrap();

    assert_eq!(new_task.task_type, "download");
    assert_eq!(new_task.params.as_deref(), Some(r#"{"image_id":42}"#));
    assert_eq!(new_task.retry_count, 0);
    assert_eq!(new_task.status, randimg_core::db::entities::task::STATUS_PENDING);
    assert_ne!(new_task.id, task.id);

    let dl_gone = randimg_core::db::query::dead_letter::get_dead_letter(&db, &dl.id)
        .await
        .unwrap();
    assert!(dl_gone.is_none());

    let remaining = randimg_core::db::query::dead_letter::list_dead_letters(&db, None, 10, 0)
        .await
        .unwrap();
    assert_eq!(remaining.len(), 0);
}

#[tokio::test]
async fn test_dead_letter_delete() {
    let db = setup_db().await;

    let task = randimg_core::db::query::task::create(
        &db, "upload", None, None, None, None, None,
    )
    .await
    .unwrap();

    let dl = randimg_core::db::query::dead_letter::insert_dead_letter(
        &db, &task.id, "upload", None, "error", 3, None,
    )
    .await
    .unwrap();

    let deleted = randimg_core::db::query::dead_letter::delete_dead_letter(&db, &dl.id)
        .await
        .unwrap();
    assert!(deleted);

    let not_found = randimg_core::db::query::dead_letter::get_dead_letter(&db, &dl.id)
        .await
        .unwrap();
    assert!(not_found.is_none());

    let already_gone = randimg_core::db::query::dead_letter::delete_dead_letter(&db, &dl.id)
        .await
        .unwrap();
    assert!(!already_gone);
}

#[tokio::test]
async fn test_dead_letter_status_constant() {
    assert_eq!(randimg_core::db::entities::task::STATUS_DEAD, "dead");
}
