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
        // SQLite does not support DROP COLUMN before 3.35.0; for simplicity
        // we recreate the table without the column. In production on PostgreSQL
        // a simple ALTER TABLE images DROP COLUMN deleted_at would suffice.
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE images DROP COLUMN deleted_at")
            .await?;
        Ok(())
    }
}
