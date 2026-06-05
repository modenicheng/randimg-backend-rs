use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Drop indexes first (IF EXISTS for safety)
        db.execute_unprepared("DROP INDEX IF EXISTS idx_task_dependencies_parent_child_unique")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_task_dependencies_parent_job_id")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_task_dependencies_child_job_id")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_task_dependencies_parent_job_id_v2")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_task_dependencies_child_job_id_v2")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_task_dependencies_pair_v2")
            .await?;

        // Drop the table
        db.execute_unprepared("DROP TABLE IF EXISTS task_dependencies")
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Recreate the table (for rollback)
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS task_dependencies (
                id SERIAL PRIMARY KEY,
                parent_job_id TEXT NOT NULL,
                child_job_id TEXT NOT NULL,
                created_at TIMESTAMP NOT NULL DEFAULT NOW()
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_task_dependencies_parent_job_id ON task_dependencies(parent_job_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_task_dependencies_child_job_id ON task_dependencies(child_job_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_task_dependencies_parent_child_unique ON task_dependencies(parent_job_id, child_job_id)",
        )
        .await?;

        Ok(())
    }
}
