use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Convert TEXT to JSONB (data is already JSON-formatted)
        db.execute_unprepared(
            "ALTER TABLE tasks ALTER COLUMN params TYPE jsonb USING params::jsonb",
        )
        .await?;

        // Add GIN index for efficient JSON queries
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_tasks_params ON tasks USING GIN (params)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Drop GIN index
        db.execute_unprepared("DROP INDEX IF EXISTS idx_tasks_params")
            .await?;

        // Convert JSONB back to TEXT
        db.execute_unprepared("ALTER TABLE tasks ALTER COLUMN params TYPE text USING params::text")
            .await?;

        Ok(())
    }
}
