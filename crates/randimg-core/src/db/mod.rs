use migration::MigratorTrait;
use sea_orm::{Database, DatabaseConnection};

pub mod entities;
pub mod query;

pub async fn init_database(database_url: &str) -> DatabaseConnection {
    let db = Database::connect(database_url)
        .await
        .expect("Failed to connect to database");

    // Enable WAL journal mode and set busy_timeout for better concurrent access.
    // WAL allows concurrent readers while a writer holds a lock, and busy_timeout
    // makes writers wait instead of immediately returning SQLITE_BUSY.
    #[cfg(feature = "db-sqlite")]
    {
        use sea_orm::{ConnectionTrait, Statement};
        for pragma in ["PRAGMA journal_mode=WAL", "PRAGMA busy_timeout=5000"] {
            db.execute(Statement::from_string(sea_orm::DatabaseBackend::Sqlite, pragma.to_string()))
                .await
                .expect("Failed to set SQLite pragma");
        }
    }

    // Run migrations
    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");

    db
}
