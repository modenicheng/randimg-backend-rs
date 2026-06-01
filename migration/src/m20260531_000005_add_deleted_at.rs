use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        #[cfg(feature = "sqlite")]
        db.execute_unprepared("ALTER TABLE images ADD COLUMN deleted_at DATETIME")
            .await?;
        #[cfg(feature = "postgres")]
        db.execute_unprepared("ALTER TABLE images ADD COLUMN deleted_at TIMESTAMPTZ")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Silently ignore failure — SQLite < 3.35.0 does not support DROP COLUMN.
        // On PostgreSQL this will succeed and remove the column.
        let db = manager.get_connection();
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN deleted_at")
            .await;
        Ok(())
    }
}
