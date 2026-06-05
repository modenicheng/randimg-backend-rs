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
                "DO $$ BEGIN CREATE TYPE task_status AS ENUM ('pending', 'queued', 'running', 'done', 'failed', 'killed', 'dead'); EXCEPTION WHEN duplicate_object THEN null; END $$",
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "DO $$ BEGIN CREATE TYPE task_type AS ENUM ('crawl', 'download', 'color_extract', 'upload', 'accessibility_check', 'discover', 'refresh_pixiv_token', 'cleanup'); EXCEPTION WHEN duplicate_object THEN null; END $$",
            )
            .await?;

        // Convert columns
        // change type, then re-set DEFAULT with explicit ENUM cast.
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE tasks ALTER COLUMN status DROP DEFAULT",
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE tasks ALTER COLUMN status TYPE task_status USING status::task_status",
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE tasks ALTER COLUMN status SET DEFAULT 'pending'::task_status",
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE tasks ALTER COLUMN task_type DROP DEFAULT",
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
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE tasks ALTER COLUMN status DROP DEFAULT")
            .await?;

        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE tasks ALTER COLUMN status TYPE text")
            .await?;

        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE tasks ALTER COLUMN status SET DEFAULT 'pending'")
            .await?;

        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE tasks ALTER COLUMN task_type DROP DEFAULT")
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
