use migration::MigratorTrait;
use sea_orm::{Database, DatabaseConnection};

pub mod entities;
pub mod query;

pub async fn init_database(database_url: &str) -> DatabaseConnection {
    let db = Database::connect(database_url)
        .await
        .expect("Failed to connect to database");

    // Run migrations
    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");

    db
}
