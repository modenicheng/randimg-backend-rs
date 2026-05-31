pub use sea_orm_migration::prelude::*;

mod m20260531_000001_create_base_tables;
mod m20260531_000002_schema_refactor;
mod m20260531_000003_color_palette;
mod m20260531_000004_rename_accessible;
mod m20260531_000005_add_deleted_at;

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
        ]
    }
}
