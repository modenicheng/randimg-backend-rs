use sea_orm::*;
use crate::db::entities::tag::{self, Entity as Tag};

pub async fn find_all(db: &DatabaseConnection) -> Result<Vec<tag::Model>, DbErr> {
    Tag::find().all(db).await
}

pub async fn find_or_create(
    db: &DatabaseConnection,
    name: &str,
    translated_name: Option<&str>,
) -> Result<tag::Model, DbErr> {
    if let Some(existing) = Tag::find()
        .filter(tag::Column::Name.eq(name))
        .one(db)
        .await?
    {
        return Ok(existing);
    }
    let model = tag::ActiveModel {
        name: Set(name.to_string()),
        translated_name: Set(translated_name.map(|s| s.to_string())),
        ..Default::default()
    };
    model.insert(db).await
}
