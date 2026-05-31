use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ========== 1. images 表新增主色 LAB 字段 ==========
        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN primary_l REAL"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN primary_a REAL"
        ).await?;

        db.execute_unprepared(
            "ALTER TABLE images ADD COLUMN primary_b REAL"
        ).await?;

        // 回填已有数据：从 colors JSON 中提取 primary_color 并转换为 LAB
        // 注意：这里只做 RGB 存储，LAB 需要应用层计算（SQL 没有 sRGB->LAB 函数）
        // 回填逻辑交给应用层的迁移脚本或后台任务

        // ========== 2. 创建 image_color_palette 表 ==========
        manager
            .create_table(
                Table::create()
                    .table(ImageColorPalette::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ImageColorPalette::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(ImageColorPalette::ImageId).integer().not_null())
                    .col(ColumnDef::new(ImageColorPalette::ColorIndex).integer().not_null())
                    .col(ColumnDef::new(ImageColorPalette::RgbR).integer().not_null())
                    .col(ColumnDef::new(ImageColorPalette::RgbG).integer().not_null())
                    .col(ColumnDef::new(ImageColorPalette::RgbB).integer().not_null())
                    .col(ColumnDef::new(ImageColorPalette::LabL).double().not_null())
                    .col(ColumnDef::new(ImageColorPalette::LabA).double().not_null())
                    .col(ColumnDef::new(ImageColorPalette::LabB).double().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_color_palette_image_id")
                            .from(ImageColorPalette::Table, ImageColorPalette::ImageId)
                            .to(Images::Table, Images::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // ========== 3. 创建索引 ==========
        // 用于 JOIN 查询
        manager
            .create_index(
                Index::create()
                    .name("idx_color_palette_image_id")
                    .table(ImageColorPalette::Table)
                    .col(ImageColorPalette::ImageId)
                    .to_owned(),
            )
            .await?;

        // 用于距离预过滤（LAB 三列联合索引）
        manager
            .create_index(
                Index::create()
                    .name("idx_color_palette_lab")
                    .table(ImageColorPalette::Table)
                    .col(ImageColorPalette::LabL)
                    .col(ImageColorPalette::LabA)
                    .col(ImageColorPalette::LabB)
                    .to_owned(),
            )
            .await?;

        // images 主色 LAB 索引
        manager
            .create_index(
                Index::create()
                    .name("idx_images_primary_lab")
                    .table(Images::Table)
                    .col(Images::PrimaryL)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(ImageColorPalette::Table).to_owned()).await?;

        let db = manager.get_connection();
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN primary_l").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN primary_a").await;
        let _ = db.execute_unprepared("ALTER TABLE images DROP COLUMN primary_b").await;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Images {
    Table, Id, PrimaryL,
}

#[derive(DeriveIden)]
enum ImageColorPalette {
    Table, Id, ImageId, ColorIndex,
    RgbR, RgbG, RgbB,
    LabL, LabA, LabB,
}
