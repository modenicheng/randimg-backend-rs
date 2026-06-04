pub use sea_orm_migration::prelude::*;

mod m20260531_000001_create_base_tables;
mod m20260531_000002_schema_refactor;
mod m20260531_000003_color_palette;
mod m20260531_000004_rename_accessible;
mod m20260531_000005_add_deleted_at;
mod m20260531_000006_create_pixiv_credentials;
mod m20260601_000001_create_task_hierarchy;
mod m20260601_000002_fix_task_hierarchy_fk;
mod m20260602_000001_add_downloaded;
mod m20260603_000001_add_illust_metadata;
mod m20260603_002_create_tasks_table;
mod m20260604_001_alter_fang_task_id_to_text;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260531_000001_create_base_tables::Migration),
            Box::new(m20260531_000002_schema_refactor::Migration),
            Box::new(m20260531_000003_color_palette::Migration),
            Box::new(m20260531_000004_rename_accessible::Migration),
            Box::new(m20260531_000005_add_deleted_at::Migration),
            Box::new(m20260531_000006_create_pixiv_credentials::Migration),
            Box::new(m20260601_000001_create_task_hierarchy::Migration),
            Box::new(m20260601_000002_fix_task_hierarchy_fk::Migration),
            Box::new(m20260602_000001_add_downloaded::Migration),
            Box::new(m20260603_000001_add_illust_metadata::Migration),
            Box::new(m20260603_002_create_tasks_table::Migration),
            Box::new(m20260604_001_alter_fang_task_id_to_text::Migration),
        ]
    }
}
