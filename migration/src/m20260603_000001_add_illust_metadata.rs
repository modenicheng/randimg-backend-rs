use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE images ADD COLUMN illust_type TEXT")
            .await?;
        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN x_restrict INTEGER NOT NULL DEFAULT 0",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN illust_ai_type INTEGER NOT NULL DEFAULT 0",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN illust_type")
            .await;
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN x_restrict")
            .await;
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN illust_ai_type")
            .await;
        Ok(())
    }
}
