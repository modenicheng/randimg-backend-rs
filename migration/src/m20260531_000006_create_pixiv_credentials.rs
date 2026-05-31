use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PixivCredentials::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PixivCredentials::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PixivCredentials::PixivUserId)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PixivCredentials::RefreshToken)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PixivCredentials::AccessToken).string())
                    .col(
                        ColumnDef::new(PixivCredentials::Status)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(PixivCredentials::Note).string())
                    .col(ColumnDef::new(PixivCredentials::LastUsedAt).date_time())
                    .col(ColumnDef::new(PixivCredentials::LastRefreshedAt).date_time())
                    .col(
                        ColumnDef::new(PixivCredentials::CreatedAt)
                            .date_time()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PixivCredentials::UpdatedAt)
                            .date_time()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_pixiv_credentials_pixiv_user_id")
                    .table(PixivCredentials::Table)
                    .col(PixivCredentials::PixivUserId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PixivCredentials::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum PixivCredentials {
    Table,
    Id,
    PixivUserId,
    RefreshToken,
    AccessToken,
    Status,
    Note,
    LastUsedAt,
    LastRefreshedAt,
    CreatedAt,
    UpdatedAt,
}
