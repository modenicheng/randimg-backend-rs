use sea_orm::*;
use crate::db::entities::author::{self, Entity as Author};

pub async fn find_or_create(
    db: &DatabaseConnection,
    name: &str,
    platform: Option<&str>,
    platform_id: Option<&str>,
) -> Result<author::Model, DbErr> {
    // First try to find by platform_id (most reliable identifier)
    if let Some(pid) = platform_id {
        if let Some(author) = Author::find()
            .filter(author::Column::PlatformId.eq(pid))
            .one(db)
            .await?
        {
            return Ok(author);
        }
    }

    // Then try to find by name
    if let Some(author) = Author::find()
        .filter(author::Column::Name.eq(name))
        .one(db)
        .await?
    {
        return Ok(author);
    }

    // Create new author
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
