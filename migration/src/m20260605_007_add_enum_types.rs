use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260605_007_add_enum_types"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create ENUM types
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TYPE task_status AS ENUM ('pending', 'queued', 'running', 'done', 'failed', 'killed', 'dead')",
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TYPE task_type AS ENUM ('crawl', 'download', 'color_extract', 'upload', 'accessibility_check', 'discover', 'refresh_pixiv_token', 'cleanup')",
            )
            .await?;

        // Convert columns (with USING clause for type casting)
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE tasks ALTER COLUMN status TYPE task_status USING status::task_status",
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE tasks ALTER COLUMN task_type TYPE task_type USING task_type::task_type",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Convert back to TEXT
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE tasks ALTER COLUMN status TYPE text")
            .await?;

        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE tasks ALTER COLUMN task_type TYPE text")
            .await?;

        // Drop ENUM types
        manager
            .get_connection()
            .execute_unprepared("DROP TYPE IF EXISTS task_status")
            .await?;

        manager
            .get_connection()
            .execute_unprepared("DROP TYPE IF EXISTS task_type")
            .await?;

        Ok(())
    }
}
