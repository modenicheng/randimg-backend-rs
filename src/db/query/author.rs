use sea_orm::*;
use crate::db::entities::author::{self, Entity as Author};

pub async fn find_or_create(
    db: &DatabaseConnection,
    name: &str,
    platform: Option<&str>,
    platform_id: Option<&str>,
) -> Result<author::Model, DbErr> {
    let existing = Author::find()
        .filter(
            author::Column::Name.eq(name)
                .or(author::Column::PlatformId.eq(platform_id.unwrap_or(""))),
        )
        .one(db)
        .await?;

    if let Some(author) = existing {
        return Ok(author);
    }

    let model = author::ActiveModel {
        name: Set(name.to_string()),
        platform: Set(platform.map(|s| s.to_string())),
        platform_id: Set(platform_id.map(|s| s.to_string())),
        ..Default::default()
    };
    model.insert(db).await
}

pub async fn count(db: &DatabaseConnection) -> Result<u64, DbErr> {
    Author::find().count(db).await
}
