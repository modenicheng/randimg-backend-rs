use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY NOT NULL,
                fang_task_id TEXT,
                task_type TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                parent_id TEXT,
                root_id TEXT,
                crawler_id INTEGER,
                image_id INTEGER,
                params TEXT,
                error_message TEXT,
                retry_count INTEGER NOT NULL DEFAULT 0,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL,
                updated_at TIMESTAMP WITH TIME ZONE NOT NULL,
                completed_at TIMESTAMP WITH TIME ZONE
            )",
        )
        .await?;

        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_tasks_task_type ON tasks(task_type)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_tasks_parent_id ON tasks(parent_id)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_tasks_root_id ON tasks(root_id)")
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS tasks").await?;
        Ok(())
    }
}
