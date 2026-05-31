use sea_orm::*;
use crate::db::entities::{
    image::{self, Entity as Image},
    author, tag,
};
use crate::config::AppConfig;

/// Get single image detail with author + tags
pub async fn find_by_id(
    db: &DatabaseConnection,
    image_id: i32,
    is_admin: bool,
    config: &AppConfig,
) -> Result<Option<serde_json::Value>, DbErr> {
    let img = Image::find_by_id(image_id)
        .find_also_related(author::Entity)
        .one(db)
        .await?;

    let Some((img, author)) = img else {
        return Ok(None);
    };

    // Non-admin cannot see accessable=false images
    if img.accessable == Some(false) && !is_admin {
        return Ok(None);
    }

    let author = author.unwrap();

    // Query associated tags
    let tags: Vec<tag::Model> = img
        .find_related(tag::Entity)
        .all(db)
        .await?;

    let tags_json: Vec<serde_json::Value> = tags
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "name": t.name,
                "translated_name": t.translated_name,
            })
        })
        .collect();

    Ok(Some(serde_json::json!({
        "id": img.id,
        "src": format!("{}{}", config.cdn_base_url, img.image_path),
        "image_path": img.image_path,
        "title": img.title,
        "source_id": img.source_id,
        "aspect_ratio": img.aspect_ratio,
        "source_url": img.source_url,
        "width": img.width,
        "height": img.height,
        "colors": img.colors,
        "author": {
            "id": author.id,
            "name": author.name,
            "platform_id": author.platform_id,
            "platform": author.platform,
        },
        "tags": tags_json,
    })))
}

/// Get random accessible image
pub async fn random_image(
    db: &DatabaseConnection,
    ratio_floor: f32,
    ratio_ceil: f32,
    tags: Option<&str>,
    config: &AppConfig,
) -> Result<Option<serde_json::Value>, DbErr> {
    // Get all accessible image IDs
    let mut query = Image::find()
        .select_only()
        .column(image::Column::Id)
        .filter(image::Column::Accessable.eq(true))
        .filter(image::Column::Uploaded.eq(true))
        .filter(image::Column::AspectRatio.gte(ratio_floor))
        .filter(image::Column::AspectRatio.lte(ratio_ceil));

    // If tags specified, join and filter
    if let Some(tag_str) = tags {
        let tag_names: Vec<&str> = tag_str.split(',').collect();
        query = query
            .join(JoinType::InnerJoin, image::Relation::ImageTagAssociation.def())
            .join(JoinType::InnerJoin, crate::db::entities::image_tag_association::Relation::Tag.def())
            .filter(
                tag::Column::Name.is_in(tag_names.clone())
                    .or(tag::Column::TranslatedName.is_in(tag_names)),
            );
    }

    let image_ids: Vec<i32> = query.into_tuple().all(db).await?;

    if image_ids.is_empty() {
        return Ok(None);
    }

    // Random selection using timestamp-based hash
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hash, Hasher};
    let s = RandomState::new();
    let mut hasher = s.build_hasher();
    std::time::Instant::now().hash(&mut hasher);
    let hash = hasher.finish();
    let idx = (hash as usize) % image_ids.len();
    let selected_id = image_ids[idx];

    find_by_id(db, selected_id, false, config).await
}

/// Paginated image list
pub async fn list_images(
    db: &DatabaseConnection,
    offset: u64,
    limit: u64,
    desc: bool,
    ratio_floor: f32,
    ratio_ceil: f32,
    author_filter: Option<&str>,
    accessable: Option<bool>,
    tags: Option<&str>,
    is_admin: bool,
    config: &AppConfig,
) -> Result<Vec<serde_json::Value>, DbErr> {
    let mut query = Image::find()
        .find_also_related(author::Entity)
        .filter(image::Column::Uploaded.eq(true))
        .filter(image::Column::Processed.eq(true))
        .filter(image::Column::AspectRatio.gte(ratio_floor))
        .filter(image::Column::AspectRatio.lte(ratio_ceil));

    // accessable filter
    if !is_admin {
        query = query.filter(image::Column::Accessable.eq(true));
    } else if let Some(acc) = accessable {
        query = query.filter(image::Column::Accessable.eq(acc));
    }

    // author filter
    if let Some(author_str) = author_filter {
        if let Ok(author_id) = author_str.parse::<i32>() {
            query = query.filter(image::Column::AuthorId.eq(author_id));
        } else {
            query = query.filter(author::Column::Name.like(format!("%{}%", author_str)));
        }
    }

    // tags filter (join-based)
    if let Some(tag_str) = tags {
        let tag_names: Vec<&str> = tag_str.split(',').collect();
        query = query
            .join(JoinType::InnerJoin, image::Relation::ImageTagAssociation.def())
            .join(JoinType::InnerJoin, crate::db::entities::image_tag_association::Relation::Tag.def())
            .filter(
                tag::Column::Name.is_in(tag_names.clone())
                    .or(tag::Column::TranslatedName.is_in(tag_names)),
            );
    }

    // Sort
    if desc {
        query = query.order_by_desc(image::Column::Id);
    } else {
        query = query.order_by_asc(image::Column::Id);
    }

    query = query.offset(offset).limit(limit);

    let results = query.all(db).await?;

    let mut output = Vec::new();
    for (img, auth) in results {
        let auth = auth.unwrap();
        let tags: Vec<tag::Model> = img.find_related(tag::Entity).all(db).await?;

        let tags_json: Vec<serde_json::Value> = tags
            .into_iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "name": t.name,
                    "translated_name": t.translated_name,
                })
            })
            .collect();

        let primary_color = img
            .colors
            .as_ref()
            .and_then(|c| c.get("primary_color"))
            .cloned();

        if is_admin {
            output.push(serde_json::json!({
                "id": img.id,
                "src": format!("{}{}", config.cdn_base_url, img.image_path),
                "title": img.title,
                "source_id": img.source_id,
                "aspect_ratio": img.aspect_ratio,
                "primary_color": primary_color,
                "accessable": img.accessable,
                "author": {
                    "id": auth.id,
                    "name": auth.name,
                    "platform_id": auth.platform_id,
                    "platform": auth.platform,
                },
                "tags": tags_json,
            }));
        } else {
            output.push(serde_json::json!({
                "id": img.id,
                "src": format!("{}{}", config.cdn_base_url, img.image_path),
                "title": img.title,
                "source_id": img.source_id,
                "aspect_ratio": img.aspect_ratio,
                "primary_color": primary_color,
                "author": auth.id,
                "tags": tags_json,
            }));
        }
    }

    Ok(output)
}

/// Get unprocessed images
pub async fn find_unprocessed(db: &DatabaseConnection) -> Result<Vec<image::Model>, DbErr> {
    Image::find()
        .filter(image::Column::Processed.eq(false))
        .filter(image::Column::Processing.eq(false))
        .all(db)
        .await
}

/// Update image fields from JSON
pub async fn update_fields(
    db: &DatabaseConnection,
    image_id: i32,
    data: serde_json::Value,
) -> Result<Option<image::Model>, DbErr> {
    let img = Image::find_by_id(image_id).one(db).await?;
    let Some(img) = img else { return Ok(None) };

    let mut active: image::ActiveModel = img.into();

    if let Some(title) = data.get("title").and_then(|v| v.as_str()) {
        active.title = Set(title.to_string());
    }
    if let Some(accessable) = data.get("accessable") {
        if accessable.is_null() {
            active.accessable = Set(None);
        } else if let Some(b) = accessable.as_bool() {
            active.accessable = Set(Some(b));
        }
    }
    if let Some(processed) = data.get("processed").and_then(|v| v.as_bool()) {
        active.processed = Set(processed);
    }
    if let Some(processing) = data.get("processing").and_then(|v| v.as_bool()) {
        active.processing = Set(processing);
    }
    if let Some(downloaded) = data.get("downloaded").and_then(|v| v.as_bool()) {
        active.downloaded = Set(downloaded);
    }
    if let Some(uploaded) = data.get("uploaded").and_then(|v| v.as_bool()) {
        active.uploaded = Set(uploaded);
    }
    if let Some(colors) = data.get("colors") {
        active.colors = Set(Some(colors.clone()));
    }

    let result = active.update(db).await?;
    Ok(Some(result))
}

/// Count accessible images
pub async fn count_accessible(db: &DatabaseConnection) -> Result<u64, DbErr> {
    Image::find()
        .filter(image::Column::Accessable.eq(true))
        .count(db)
        .await
}

/// Create image record
pub async fn create_image(
    db: &DatabaseConnection,
    data: &serde_json::Value,
) -> Result<image::Model, DbErr> {
    let model = image::ActiveModel {
        title: Set(data["title"].as_str().unwrap_or("").to_string()),
        image_path: Set(data["image_path"].as_str().unwrap_or("").to_string()),
        source_url: Set(data["source_url"].as_str().map(|s| s.to_string())),
        source_id: Set(data["source_id"].as_i64().map(|v| v as i32)),
        source_image_url: Set(data["source_image_url"].as_str().map(|s| s.to_string())),
        author_id: Set(data["author_id"].as_i64().unwrap_or(0) as i32),
        width: Set(data["width"].as_i64().unwrap_or(0) as i32),
        height: Set(data["height"].as_i64().unwrap_or(0) as i32),
        aspect_ratio: Set(data["aspect_ratio"].as_f64().unwrap_or(0.0) as f32),
        colors: Set(data.get("colors").cloned()),
        ..Default::default()
    };
    model.insert(db).await
}
