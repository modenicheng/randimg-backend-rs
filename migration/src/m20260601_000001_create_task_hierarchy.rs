use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create task_dependencies table for tracking parent-child relationships
        manager
            .create_table(
                Table::create()
                    .table(TaskDependencies::Table)
                    .if_not_exists()
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
                    // Foreign keys to apalis.jobs table
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_task_dependencies_parent_job_id")
                            .from(TaskDependencies::Table, TaskDependencies::ParentJobId)
                            .to(ApalisJobs::Table, ApalisJobs::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_task_dependencies_child_job_id")
                            .from(TaskDependencies::Table, TaskDependencies::ChildJobId)
                            .to(ApalisJobs::Table, ApalisJobs::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create indexes for efficient queries
        manager
            .create_index(
                Index::create()
                    .name("idx_task_dependencies_parent_job_id")
                    .table(TaskDependencies::Table)
                    .col(TaskDependencies::ParentJobId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_task_dependencies_child_job_id")
                    .table(TaskDependencies::Table)
                    .col(TaskDependencies::ChildJobId)
                    .to_owned(),
            )
            .await?;

        // Unique constraint: each (parent, child) pair should be unique
        manager
            .create_index(
                Index::create()
                    .name("idx_task_dependencies_parent_child_unique")
                    .table(TaskDependencies::Table)
                    .col(TaskDependencies::ParentJobId)
                    .col(TaskDependencies::ChildJobId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TaskDependencies::Table).to_owned())
            .await?;
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

#[derive(DeriveIden)]
enum ApalisJobs {
    Table,
    #[cfg(feature = "sqlite")]
    Id,
    #[cfg(feature = "postgres")]
    Id,
}
