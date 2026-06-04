use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // images.primary_l/a/b 从 REAL (FLOAT4) 改为 DOUBLE PRECISION (FLOAT8)
        // 与 SeaORM entity 的 f64 类型匹配
        db.execute_unprepared(
            "ALTER TABLE images ALTER COLUMN primary_l TYPE DOUBLE PRECISION"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ALTER COLUMN primary_a TYPE DOUBLE PRECISION"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ALTER COLUMN primary_b TYPE DOUBLE PRECISION"
        ).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "ALTER TABLE images ALTER COLUMN primary_l TYPE REAL"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ALTER COLUMN primary_a TYPE REAL"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ALTER COLUMN primary_b TYPE REAL"
        ).await?;

        Ok(())
    }
}
