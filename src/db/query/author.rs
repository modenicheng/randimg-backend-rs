use crate::db::entities::author::{self, Entity as Author};
use sea_orm::*;

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

pub async fn find_all(
    db: &DatabaseConnection,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<author::Model>, DbErr> {
    let mut query = Author::find().order_by_asc(author::Column::Id);
    if let Some(l) = limit {
        query = query.limit(l);
    }
    if let Some(o) = offset {
        query = query.offset(o);
    }
    query.all(db).await
}

pub async fn find_by_id(
    db: &DatabaseConnection,
    author_id: i32,
) -> Result<Option<author::Model>, DbErr> {
    Author::find_by_id(author_id).one(db).await
}
