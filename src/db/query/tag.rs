use crate::db::entities::tag::{self, Entity as Tag};
use sea_orm::*;

pub async fn find_all(
    db: &DatabaseConnection,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<tag::Model>, DbErr> {
    let mut query = Tag::find().order_by_asc(tag::Column::Id);
    if let Some(l) = limit {
        query = query.limit(l);
    }
    if let Some(o) = offset {
        query = query.offset(o);
    }
    query.all(db).await
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
        // Update translated_name if existing tag has None but new value is Some
        if existing.translated_name.is_none() {
            if let Some(new_translated) = translated_name {
                let mut active: tag::ActiveModel = existing.into();
                active.translated_name = Set(Some(new_translated.to_string()));
                return active.update(db).await;
            }
        }
        return Ok(existing);
    }
    let model = tag::ActiveModel {
        name: Set(name.to_string()),
        translated_name: Set(translated_name.map(|s| s.to_string())),
        ..Default::default()
    };
    model.insert(db).await
}

pub async fn update_tag(
    db: &DatabaseConnection,
    tag_id: i32,
    translated_name: Option<&str>,
) -> Result<Option<tag::Model>, DbErr> {
    let t = Tag::find_by_id(tag_id).one(db).await?;
    let Some(t) = t else { return Ok(None) };
    let mut active: tag::ActiveModel = t.into();
    active.translated_name = Set(translated_name.map(|s| s.to_string()));
    let updated = active.update(db).await?;
    Ok(Some(updated))
}
