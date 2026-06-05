use chrono::{TimeZone, Utc};
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, EntityTrait, Set};

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
async fn test_cleanup_deletes_old_done_tasks() {
    let db = setup_db().await;

    let old_time = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();

    let task = randimg_core::db::query::task::create(
        &db, "download", None, None, None, None, None,
    )
    .await
    .unwrap();

    // Mark as done with old completed_at
    {
        use randimg_core::db::entities::task::{self, Entity as Task};
        if let Some(t) = Task::find_by_id(task.id.clone()).one(&db).await.unwrap() {
            let mut active: task::ActiveModel = t.into();
            active.status = Set(task::STATUS_DONE.to_string());
            active.completed_at = Set(Some(old_time.into()));
            active.update(&db).await.unwrap();
        }
    }

    let deleted = randimg_core::db::query::task::delete_by_statuses_and_older_than(
        &db,
        &[randimg_core::db::entities::task::STATUS_DONE],
        24,
    )
    .await
    .unwrap();

    assert_eq!(deleted, 1);

    let remaining = randimg_core::db::query::task::find_by_id(&db, &task.id)
        .await
        .unwrap();
    assert!(remaining.is_none());
}

#[tokio::test]
async fn test_cleanup_deletes_old_dead_letters() {
    let db = setup_db().await;

    let task = randimg_core::db::query::task::create(
        &db, "crawl", None, None, None, None, None,
    )
    .await
    .unwrap();

    let dl = randimg_core::db::query::dead_letter::insert_dead_letter(
        &db,
        &task.id,
        "crawl",
        None,
        "permanent failure",
        3,
        None,
    )
    .await
    .unwrap();

    // Override created_at to an old time
    {
        use randimg_core::db::entities::dead_letter::{self, Entity as DeadLetter};
        let old_time = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        if let Some(entry) = DeadLetter::find_by_id(dl.id.clone()).one(&db).await.unwrap() {
            let mut active: dead_letter::ActiveModel = entry.into();
            active.created_at = Set(old_time.into());
            active.update(&db).await.unwrap();
        }
    }

    let deleted = randimg_core::db::query::dead_letter::delete_older_than(&db, 24)
        .await
        .unwrap();

    assert_eq!(deleted, 1);

    let remaining = randimg_core::db::query::dead_letter::get_dead_letter(&db, &dl.id)
        .await
        .unwrap();
    assert!(remaining.is_none());
}

#[tokio::test]
async fn test_cleanup_preserves_recent_tasks() {
    let db = setup_db().await;

    // Create a task marked as done with recent completed_at (now)
    let task = randimg_core::db::query::task::create(
        &db, "upload", None, None, None, None, None,
    )
    .await
    .unwrap();

    randimg_core::db::query::task::update_status(&db, &task.id, "done")
        .await
        .unwrap();

    // Try to delete with 24h TTL — should not delete the just-completed task
    let deleted = randimg_core::db::query::task::delete_by_statuses_and_older_than(
        &db,
        &["done"],
        24,
    )
    .await
    .unwrap();

    assert_eq!(deleted, 0);

    let remaining = randimg_core::db::query::task::find_by_id(&db, &task.id)
        .await
        .unwrap();
    assert!(remaining.is_some());
}
