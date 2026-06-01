use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop the broken table (created with FK to non-existent apalis_jobs)
        // and recreate it without foreign keys.  Data loss is acceptable — the
        // table has 0 rows because every INSERT has been failing at runtime.
        manager
            .drop_table(Table::drop().table(TaskDependencies::Table).to_owned())
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(TaskDependencies::Table)
                    .col(
                        ColumnDef::new(TaskDependencies::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TaskDependencies::ParentJobId)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TaskDependencies::ChildJobId)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TaskDependencies::CreatedAt)
                            .date_time()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Re-create indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_task_dependencies_parent_job_id_v2")
                    .table(TaskDependencies::Table)
                    .col(TaskDependencies::ParentJobId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_task_dependencies_child_job_id_v2")
                    .table(TaskDependencies::Table)
                    .col(TaskDependencies::ChildJobId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_task_dependencies_pair_v2")
                    .table(TaskDependencies::Table)
                    .col(TaskDependencies::ParentJobId)
                    .col(TaskDependencies::ChildJobId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum TaskDependencies {
    Table,
    Id,
    ParentJobId,
    ChildJobId,
    CreatedAt,
}
