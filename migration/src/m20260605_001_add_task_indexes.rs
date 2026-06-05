use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Index on fang_task_id for reverse lookups from Fang to custom tasks
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_tasks_fang_task_id ON tasks(fang_task_id)",
        )
        .await?;

        // Index on status for filtering by status (may already exist, IF NOT EXISTS is safe)
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status)")
            .await?;

        // Composite index on (root_id, parent_id) for tree queries
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_tasks_root_parent ON tasks(root_id, parent_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP INDEX IF EXISTS idx_tasks_fang_task_id")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_tasks_root_parent")
            .await?;

        // idx_tasks_status is not dropped here since it was created in a prior migration

        Ok(())
    }
}
