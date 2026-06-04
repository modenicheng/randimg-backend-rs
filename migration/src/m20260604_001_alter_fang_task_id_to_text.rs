use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Convert fang_task_id from BIGINT to TEXT (UUID string).
        // Existing i64 values are cast to text; new inserts store UUID strings directly.
        db.execute_unprepared(
            "ALTER TABLE tasks ALTER COLUMN fang_task_id TYPE TEXT USING fang_task_id::TEXT",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE tasks ALTER COLUMN fang_task_id TYPE BIGINT USING fang_task_id::BIGINT",
        )
        .await?;
        Ok(())
    }
}
