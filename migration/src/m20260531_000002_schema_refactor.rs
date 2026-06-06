use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ========== 1. images 新增业务字段 ==========
        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN is_public BOOLEAN NOT NULL DEFAULT false",
        )
        .await?;

        db.execute_unprepared("ALTER TABLE images ADD COLUMN avatar_available BOOLEAN")
            .await?;

        db.execute_unprepared("ALTER TABLE images ADD COLUMN source_created_at TIMESTAMPTZ")
            .await?;

        db.execute_unprepared("ALTER TABLE images ADD COLUMN total_view BIGINT NOT NULL DEFAULT 0")
            .await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN total_bookmarks BIGINT NOT NULL DEFAULT 0",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN total_comments BIGINT NOT NULL DEFAULT 0",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN fetched_times INTEGER NOT NULL DEFAULT 0",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()",
        )
        .await?;

        // ========== 2. 数据迁移：从旧字段计算 is_public ==========
        // is_public = uploaded AND processed AND accessable != 0
        // (accessable IS NULL 视为未审核，不公开；accessable = 0 视为拒绝)
        db.execute_unprepared(
            "UPDATE images SET is_public = CASE
                WHEN uploaded = true AND processed = true AND (accessable IS NULL OR accessable = true)
                THEN true ELSE false END"
        ).await?;

        // ========== 3. 移除旧字段（SQLite 3.35+ 支持 DROP COLUMN） ==========
        // 如果 SQLite 版本不支持，这些语句会失败，旧字段保留但不再被代码使用
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN uploaded")
            .await;

        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN downloaded")
            .await;

        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN processed")
            .await;

        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN processing")
            .await;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // 恢复 images 旧字段（如果之前被删除了，重新加回来）
        let _ = db
            .execute_unprepared(
                "ALTER TABLE images ADD COLUMN uploaded BOOLEAN NOT NULL DEFAULT false",
            )
            .await;

        let _ = db
            .execute_unprepared(
                "ALTER TABLE images ADD COLUMN downloaded BOOLEAN NOT NULL DEFAULT false",
            )
            .await;

        let _ = db
            .execute_unprepared(
                "ALTER TABLE images ADD COLUMN processed BOOLEAN NOT NULL DEFAULT false",
            )
            .await;

        let _ = db
            .execute_unprepared(
                "ALTER TABLE images ADD COLUMN processing BOOLEAN NOT NULL DEFAULT false",
            )
            .await;

        // 恢复旧字段数据
        db.execute_unprepared(
            "UPDATE images SET
                uploaded = CASE WHEN is_public = true THEN true ELSE false END,
                processed = CASE WHEN is_public = true THEN true ELSE false END
        ",
        )
        .await?;

        // 删除新增列（SQLite 3.35+ 支持）
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN is_public")
            .await;
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN avatar_available")
            .await;
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN source_created_at")
            .await;
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN total_view")
            .await;
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN total_bookmarks")
            .await;
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN total_comments")
            .await;
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN fetched_times")
            .await;
        let _ = db
            .execute_unprepared("ALTER TABLE images DROP COLUMN created_at")
            .await;

        Ok(())
    }
}
