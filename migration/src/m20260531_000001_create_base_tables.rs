use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // authors
        manager
            .create_table(
                Table::create()
                    .table(Authors::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Authors::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Authors::Name).string().not_null())
                    .col(ColumnDef::new(Authors::Platform).string())
                    .col(ColumnDef::new(Authors::PlatformId).string())
                    .col(ColumnDef::new(Authors::Homepage).string())
                    .to_owned(),
            )
            .await?;

        // tags
        manager
            .create_table(
                Table::create()
                    .table(Tags::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Tags::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Tags::Name).string().not_null())
                    .col(ColumnDef::new(Tags::TranslatedName).string())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tags_name")
                    .table(Tags::Table)
                    .col(Tags::Name)
                    .to_owned(),
            )
            .await?;

        // images
        manager
            .create_table(
                Table::create()
                    .table(Images::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Images::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Images::Title).string().not_null().default(""))
                    .col(ColumnDef::new(Images::ImagePath).string().not_null())
                    .col(ColumnDef::new(Images::SourceUrl).string())
                    .col(ColumnDef::new(Images::SourceId).integer())
                    .col(ColumnDef::new(Images::SourceImageUrl).string())
                    .col(ColumnDef::new(Images::AuthorId).integer().not_null())
                    .col(ColumnDef::new(Images::Width).integer().not_null().default(0))
                    .col(ColumnDef::new(Images::Height).integer().not_null().default(0))
                    .col(ColumnDef::new(Images::AspectRatio).float().not_null().default(0.0))
                    .col(ColumnDef::new(Images::Colors).json())
                    .col(ColumnDef::new(Images::Accessable).boolean())
                    .col(ColumnDef::new(Images::Uploaded).boolean().not_null().default(false))
                    .col(ColumnDef::new(Images::Downloaded).boolean().not_null().default(false))
                    .col(ColumnDef::new(Images::Processed).boolean().not_null().default(false))
                    .col(ColumnDef::new(Images::Processing).boolean().not_null().default(false))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_images_author_id")
                            .from(Images::Table, Images::AuthorId)
                            .to(Authors::Table, Authors::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // image_tag_association (many-to-many join)
        manager
            .create_table(
                Table::create()
                    .table(ImageTagAssociation::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ImageTagAssociation::ImageId).integer().not_null())
                    .col(ColumnDef::new(ImageTagAssociation::TagId).integer().not_null())
                    .primary_key(
                        Index::create()
                            .col(ImageTagAssociation::ImageId)
                            .col(ImageTagAssociation::TagId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ImageTagAssociation::Table, ImageTagAssociation::ImageId)
                            .to(Images::Table, Images::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ImageTagAssociation::Table, ImageTagAssociation::TagId)
                            .to(Tags::Table, Tags::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // admins
        manager
            .create_table(
                Table::create()
                    .table(Admins::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Admins::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Admins::Username).string().not_null())
                    .col(ColumnDef::new(Admins::Password).string().not_null())
                    .col(ColumnDef::new(Admins::IsSuperuser).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_admins_username")
                    .table(Admins::Table)
                    .col(Admins::Username)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // crawlers
        manager
            .create_table(
                Table::create()
                    .table(Crawlers::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Crawlers::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Crawlers::TaskName).string().not_null())
                    .col(ColumnDef::new(Crawlers::StartTime).date_time())
                    .col(ColumnDef::new(Crawlers::EndTime).date_time())
                    .col(ColumnDef::new(Crawlers::CrawlType).integer().not_null().default(0))
                    .col(ColumnDef::new(Crawlers::Status).integer().not_null().default(0))
                    .col(ColumnDef::new(Crawlers::TotalPages).integer())
                    .col(ColumnDef::new(Crawlers::ProcessedPages).integer())
                    .col(ColumnDef::new(Crawlers::TargetUserId).string())
                    .col(ColumnDef::new(Crawlers::TargetStartDate).date_time())
                    .col(ColumnDef::new(Crawlers::TargetEndDate).date_time())
                    .col(ColumnDef::new(Crawlers::TargetSearchPrompt).string())
                    .to_owned(),
            )
            .await?;

        // background_tasks (new - replaces in-memory deque)
        manager
            .create_table(
                Table::create()
                    .table(BackgroundTasks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(BackgroundTasks::Id).string().not_null().primary_key())
                    .col(ColumnDef::new(BackgroundTasks::TaskType).string().not_null())
                    .col(ColumnDef::new(BackgroundTasks::Payload).json().not_null())
                    .col(ColumnDef::new(BackgroundTasks::Status).string().not_null().default("pending"))
                    .col(ColumnDef::new(BackgroundTasks::Priority).integer().not_null().default(0))
                    .col(ColumnDef::new(BackgroundTasks::RetryCount).integer().not_null().default(0))
                    .col(ColumnDef::new(BackgroundTasks::MaxRetries).integer().not_null().default(3))
                    .col(ColumnDef::new(BackgroundTasks::CreatedAt).date_time().not_null())
                    .col(ColumnDef::new(BackgroundTasks::StartedAt).date_time())
                    .col(ColumnDef::new(BackgroundTasks::FinishedAt).date_time())
                    .col(ColumnDef::new(BackgroundTasks::LastError).string())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(BackgroundTasks::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Crawlers::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Admins::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ImageTagAssociation::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Images::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Tags::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Authors::Table).to_owned()).await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Authors {
    Table, Id, Name, Platform, PlatformId, Homepage,
}

#[derive(DeriveIden)]
enum Tags {
    Table, Id, Name, TranslatedName,
}

#[derive(DeriveIden)]
enum Images {
    Table, Id, Title, ImagePath, SourceUrl, SourceId, SourceImageUrl,
    AuthorId, Width, Height, AspectRatio, Colors, Accessable,
    Uploaded, Downloaded, Processed, Processing,
}

#[derive(DeriveIden)]
enum ImageTagAssociation {
    Table, ImageId, TagId,
}

#[derive(DeriveIden)]
enum Admins {
    Table, Id, Username, Password, IsSuperuser,
}

#[derive(DeriveIden)]
enum Crawlers {
    Table, Id, TaskName, StartTime, EndTime, CrawlType, Status,
    TotalPages, ProcessedPages, TargetUserId, TargetStartDate,
    TargetEndDate, TargetSearchPrompt,
}

#[derive(DeriveIden)]
enum BackgroundTasks {
    Table, Id, TaskType, Payload, Status, Priority,
    RetryCount, MaxRetries, CreatedAt, StartedAt, FinishedAt, LastError,
}
