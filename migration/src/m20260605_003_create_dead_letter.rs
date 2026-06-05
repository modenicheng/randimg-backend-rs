use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS dead_letter (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                task_id UUID NOT NULL,
                task_type TEXT NOT NULL,
                params TEXT,
                error_message TEXT NOT NULL,
                retry_count INTEGER NOT NULL DEFAULT 0,
                failure_history JSONB DEFAULT '[]',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_dead_letter_task_id ON dead_letter(task_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_dead_letter_task_type ON dead_letter(task_type)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP INDEX IF EXISTS idx_dead_letter_task_type")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_dead_letter_task_id")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS dead_letter")
            .await?;

        Ok(())
    }
}
