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
mod m20260604_002_fix_primary_lab_type;
mod m20260605_001_add_task_indexes;
mod m20260605_002_drop_task_dependencies;
mod m20260605_003_create_dead_letter;
mod m20260605_004_add_task_priority;
mod m20260605_005_add_task_progress;
mod m20260605_006_params_text_to_jsonb;
mod m20260605_007_add_enum_types;
mod m20260606_000001_add_unique_tag_name;
mod m20260606_000002_json_to_jsonb;

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
            Box::new(m20260604_002_fix_primary_lab_type::Migration),
            Box::new(m20260605_001_add_task_indexes::Migration),
            Box::new(m20260605_002_drop_task_dependencies::Migration),
            Box::new(m20260605_003_create_dead_letter::Migration),
            Box::new(m20260605_004_add_task_priority::Migration),
            Box::new(m20260605_005_add_task_progress::Migration),
            Box::new(m20260605_006_params_text_to_jsonb::Migration),
            Box::new(m20260605_007_add_enum_types::Migration),
            Box::new(m20260606_000001_add_unique_tag_name::Migration),
            Box::new(m20260606_000002_json_to_jsonb::Migration),
        ]
    }
}
