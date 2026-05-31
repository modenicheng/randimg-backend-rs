use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ========== 1. background_tasks 新增 image_id / image_path 列 ==========
        db.execute_unprepared(
            "ALTER TABLE background_tasks ADD COLUMN image_id INTEGER"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE background_tasks ADD COLUMN image_path TEXT"
        ).await?;

        // 回填 background_tasks.image_id（从 payload JSON 中提取）
        db.execute_unprepared(
            "UPDATE background_tasks SET image_id = CAST(json_extract(payload, '$.image_id') AS INTEGER)
             WHERE json_extract(payload, '$.image_id') IS NOT NULL"
        ).await?;

        // 回填 background_tasks.image_path（从 payload JSON 中提取）
        db.execute_unprepared(
            "UPDATE background_tasks SET image_path = json_extract(payload, '$.image_path')
             WHERE json_extract(payload, '$.image_path') IS NOT NULL"
        ).await?;

        // ========== 2. images 新增业务字段 ==========
        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN is_public BOOLEAN NOT NULL DEFAULT 0"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN avatar_available BOOLEAN"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN source_created_at DATETIME"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN total_view BIGINT NOT NULL DEFAULT 0"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN total_bookmarks BIGINT NOT NULL DEFAULT 0"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN total_comments BIGINT NOT NULL DEFAULT 0"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN fetched_times INTEGER NOT NULL DEFAULT 0"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP"
        ).await?;

        // ========== 3. 数据迁移：从旧字段计算 is_public ==========
        // is_public = uploaded AND processed AND accessable != 0
        // (accessable IS NULL 视为未审核，不公开；accessable = 0 视为拒绝)
        db.execute_unprepared(
            "UPDATE images SET is_public = CASE
                WHEN uploaded = 1 AND processed = 1 AND (accessable IS NULL OR accessable = 1)
                THEN 1 ELSE 0 END"
        ).await?;

        // ========== 4. 移除旧字段（SQLite 3.35+ 支持 DROP COLUMN） ==========
        // 如果 SQLite 版本不支持，这些语句会失败，旧字段保留但不再被代码使用
        let _ = db.execute_unprepared(
            "ALTER TABLE images DROP COLUMN uploaded"
        ).await;

        let _ = db.execute_unprepared(
            "ALTER TABLE images DROP COLUMN downloaded"
        ).await;

        let _ = db.execute_unprepared(
            "ALTER TABLE images DROP COLUMN processed"
        ).await;

        let _ = db.execute_unprepared(
            "ALTER TABLE images DROP COLUMN processing"
        ).await;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // 恢复 images 旧字段（如果之前被删除了，重新加回来）
        let _ = db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN uploaded BOOLEAN NOT NULL DEFAULT 0"
        ).await;

        let _ = db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN downloaded BOOLEAN NOT NULL DEFAULT 0"
        ).await;

        let _ = db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN processed BOOLEAN NOT NULL DEFAULT 0"
        ).await;

        let _ = db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN processing BOOLEAN NOT NULL DEFAULT 0"
        ).await;

        // 恢复旧字段数据
        db.execute_unprepared(
            "UPDATE images SET
                uploaded = CASE WHEN is_public = 1 THEN 1 ELSE 0 END,
                processed = CASE WHEN is_public = 1 THEN 1 ELSE 0 END
        ").await?;

        // 删除新增列（SQLite 3.35+ 支持）
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN is_public").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN avatar_available").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN source_created_at").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN total_view").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN total_bookmarks").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN total_comments").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN fetched_times").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN created_at").await;

        // 删除 background_tasks 新增列
        let _ = db.execute_unprepared("ALTER TABLE background_tasks DROP COLUMN image_id").await;
        let _ = db.execute_unprepared("ALTER TABLE background_tasks DROP COLUMN image_path").await;

        Ok(())
    }
}
