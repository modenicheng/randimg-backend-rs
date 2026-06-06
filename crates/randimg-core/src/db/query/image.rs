use crate::config::AppConfig;
use crate::db::entities::{
    author,
    image::{self, Entity as Image},
    image_color_palette, tag,
};
use sea_orm::*;
use sea_orm::sea_query::Expr;

/// Filter out soft-deleted images from a query.
fn exclude_deleted(query: Select<Image>) -> Select<Image> {
    query.filter(image::Column::DeletedAt.is_null())
}

/// Calculate popularity score from image fields.
/// score = (V + B*3 + C*2) / (T_crawl_age_hours + 2)^1.8
/// T_crawl_age = created_at - source_created_at (hours)
/// If source_created_at is None, T defaults to 168 (1 week).
fn popularity_score(img: &image::Model) -> f64 {
    let v = img.total_view as f64;
    let b = img.total_bookmarks as f64;
    let c = img.total_comments as f64;
    let numerator = v + b * 3.0 + c * 2.0;

    let t_hours = match img.source_created_at {
        Some(src) => {
            let diff = img.created_at.signed_duration_since(src);
            diff.num_hours().max(0) as f64
        }
        None => 168.0, // default 1 week
    };

    numerator / (t_hours + 2.0).powf(1.8)
}

/// Get single image detail with author + tags
pub async fn find_by_id(
    db: &DatabaseConnection,
    image_id: i32,
    is_admin: bool,
    config: &AppConfig,
) -> Result<Option<serde_json::Value>, DbErr> {
    let img = Image::find()
        .filter(image::Column::Id.eq(image_id))
        .filter(image::Column::DeletedAt.is_null())
        .find_also_related(author::Entity)
        .one(db)
        .await?;

    let Some((img, author)) = img else {
        return Ok(None);
    };

    // Non-admin cannot see accessible=false images
    if img.accessible == Some(false) && !is_admin {
        return Ok(None);
    }

    let Some(author) = author else {
        return Ok(None);
    };

    // Query associated tags (explicit join to avoid ambiguous column names)
    let tags: Vec<tag::Model> = {
        use crate::db::entities::image_tag_association::{self, Entity as AssocEntity};
        let assocs = AssocEntity::find()
            .filter(image_tag_association::Column::ImageId.eq(img.id))
            .find_also_related(tag::Entity)
            .all(db)
            .await?;
        assocs.into_iter().filter_map(|(_, tag)| tag).collect()
    };

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

    // Query color palette
    let palette: Vec<image_color_palette::Model> = img
        .find_related(image_color_palette::Entity)
        .order_by_asc(image_color_palette::Column::ColorIndex)
        .all(db)
        .await?;

    let palette_json: Vec<serde_json::Value> = palette
        .iter()
        .map(|p| {
            serde_json::json!({
                "rgb": [p.rgb_r, p.rgb_g, p.rgb_b],
                "lab": [p.lab_l, p.lab_a, p.lab_b],
            })
        })
        .collect();

    let primary_color = if img.primary_l.is_some() {
        Some(serde_json::json!({
            "rgb": img.colors.as_ref().and_then(|c| c.get("primary_color")).cloned(),
            "lab": [img.primary_l, img.primary_a, img.primary_b],
        }))
    } else {
        img.colors
            .as_ref()
            .and_then(|c| c.get("primary_color"))
            .cloned()
    };

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
        "primary_color": primary_color,
        "colors": palette_json,
        "source_created_at": img.source_created_at,
        "total_view": img.total_view,
        "total_bookmarks": img.total_bookmarks,
        "total_comments": img.total_comments,
        "accessible": img.accessible,
        "is_public": img.is_public,
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
    width_floor: i32,
    width_ceil: i32,
    height_floor: i32,
    height_ceil: i32,
    author_filter: Option<&str>,
    tags: Option<&str>,
    config: &AppConfig,
) -> Result<Option<serde_json::Value>, DbErr> {
    let mut query = exclude_deleted(Image::find())
        .filter(image::Column::IsPublic.eq(true))
        .filter(image::Column::Accessible.ne(Some(false)))
        .filter(image::Column::AspectRatio.gte(ratio_floor))
        .filter(image::Column::AspectRatio.lte(ratio_ceil))
        .filter(image::Column::Width.gte(width_floor))
        .filter(image::Column::Width.lte(width_ceil))
        .filter(image::Column::Height.gte(height_floor))
        .filter(image::Column::Height.lte(height_ceil));

    // author filter (join authors for name matching, but keep Select<Image> type)
    if let Some(author_str) = author_filter {
        if let Ok(author_id) = author_str.parse::<i32>() {
            query = query.filter(image::Column::AuthorId.eq(author_id));
        } else {
            query = query
                .join(JoinType::InnerJoin, image::Relation::Author.def())
                .filter(author::Column::Name.like(format!("%{}%", author_str)));
        }
    }

    // If tags specified, join and filter; .distinct() prevents duplicate
    // rows when an image matches multiple tags (images.colors is jsonb)
    if let Some(tag_str) = tags {
        let tag_names: Vec<&str> = tag_str.split(',').collect();
        query = query
            .join(
                JoinType::InnerJoin,
                image::Relation::ImageTagAssociation.def(),
            )
            .join(
                JoinType::InnerJoin,
                crate::db::entities::image_tag_association::Relation::Tag.def(),
            )
            .filter(
                tag::Column::Name
                    .is_in(tag_names.clone())
                    .or(tag::Column::TranslatedName.is_in(tag_names)),
            )
            .distinct();
    }

    // Use database-level random selection (RANDOM() works on both SQLite and PostgreSQL)
    let img = query.order_by_asc(Expr::cust("RANDOM()")).one(db).await?;

    let Some(img) = img else {
        return Ok(None);
    };

    find_by_id(db, img.id, false, config).await
}

/// Paginated image list
pub async fn list_images(
    db: &DatabaseConnection,
    offset: u64,
    limit: u64,
    desc: bool,
    sort_by: &str,
    ratio_floor: f32,
    ratio_ceil: f32,
    width_floor: i32,
    width_ceil: i32,
    height_floor: i32,
    height_ceil: i32,
    author_filter: Option<&str>,
    accessible: Option<bool>,
    tags: Option<&str>,
    is_admin: bool,
    config: &AppConfig,
) -> Result<Vec<serde_json::Value>, DbErr> {
    let mut query = exclude_deleted(Image::find())
        .find_also_related(author::Entity)
        .filter(image::Column::IsPublic.eq(true))
        .filter(image::Column::AspectRatio.gte(ratio_floor))
        .filter(image::Column::AspectRatio.lte(ratio_ceil))
        .filter(image::Column::Width.gte(width_floor))
        .filter(image::Column::Width.lte(width_ceil))
        .filter(image::Column::Height.gte(height_floor))
        .filter(image::Column::Height.lte(height_ceil));

    // accessible filter (admin can override)
    if !is_admin {
        query = query.filter(image::Column::Accessible.ne(Some(false)));
    } else if let Some(acc) = accessible {
        query = query.filter(image::Column::Accessible.eq(acc));
    }

    // author filter
    if let Some(author_str) = author_filter {
        if let Ok(author_id) = author_str.parse::<i32>() {
            query = query.filter(image::Column::AuthorId.eq(author_id));
        } else {
            query = query.filter(author::Column::Name.like(format!("%{}%", author_str)));
        }
    }

    // tags filter (join-based; .distinct() prevents duplicate rows
    // when an image matches multiple tags or the tags table has duplicate names)
    if let Some(tag_str) = tags {
        let tag_names: Vec<&str> = tag_str.split(',').collect();
        query = query
            .join(
                JoinType::InnerJoin,
                image::Relation::ImageTagAssociation.def(),
            )
            .join(
                JoinType::InnerJoin,
                crate::db::entities::image_tag_association::Relation::Tag.def(),
            )
            .filter(
                tag::Column::Name
                    .is_in(tag_names.clone())
                    .or(tag::Column::TranslatedName.is_in(tag_names)),
            )
            .distinct();
    }

    // Popularity sort is handled at application level; others use DB ordering
    let is_popularity = sort_by == "popularity";

    if !is_popularity {
        let sort_column = match sort_by {
            "width" => image::Column::Width,
            "height" => image::Column::Height,
            "aspect_ratio" => image::Column::AspectRatio,
            "source_created_at" => image::Column::SourceCreatedAt,
            "created_at" => image::Column::CreatedAt,
            _ => image::Column::Id,
        };
        if desc {
            query = query.order_by_desc(sort_column);
        } else {
            query = query.order_by_asc(sort_column);
        }
        query = query.offset(offset).limit(limit);
    }

    let results = query.all(db).await?;

    // For popularity sort: compute scores in Rust, sort, then paginate
    let results: Vec<_> = if is_popularity {
        let mut scored: Vec<(f64, image::Model, Option<author::Model>)> = results
            .into_iter()
            .map(|(img, auth)| (popularity_score(&img), img, auth))
            .collect();
        if desc {
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        } else {
            scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        }
        scored
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|(_, img, auth)| (img, auth))
            .collect()
    } else {
        results
    };

    // Batch fetch tags for all images (avoid N+1)
    let image_ids: Vec<i32> = results.iter().map(|(img, _)| img.id).collect();

    let tag_rows: Vec<(i32, tag::Model)> = if image_ids.is_empty() {
        Vec::new()
    } else {
        use crate::db::entities::image_tag_association::{self, Entity as AssocEntity};
        let assocs = AssocEntity::find()
            .filter(image_tag_association::Column::ImageId.is_in(image_ids))
            .find_also_related(tag::Entity)
            .all(db)
            .await?;

        assocs
            .into_iter()
            .filter_map(|(assoc, tag)| tag.map(|t| (assoc.image_id, t)))
            .collect()
    };

    // Group tags by image_id
    let mut tags_by_image: std::collections::HashMap<i32, Vec<serde_json::Value>> =
        std::collections::HashMap::new();
    for (image_id, t) in tag_rows {
        tags_by_image
            .entry(image_id)
            .or_default()
            .push(serde_json::json!({
                "id": t.id,
                "name": t.name,
                "translated_name": t.translated_name,
            }));
    }

    let mut output = Vec::new();
    for (img, auth) in results {
        let Some(auth) = auth else { continue };
        let tags_json = tags_by_image.remove(&img.id).unwrap_or_default();

        let primary_color = if img.primary_l.is_some() {
            Some(serde_json::json!({
                "rgb": img.colors.as_ref().and_then(|c| c.get("primary_color")).cloned(),
                "lab": [img.primary_l, img.primary_a, img.primary_b],
            }))
        } else {
            img.colors
                .as_ref()
                .and_then(|c| c.get("primary_color"))
                .cloned()
        };

        if is_admin {
            output.push(serde_json::json!({
                "id": img.id,
                "src": format!("{}{}", config.cdn_base_url, img.image_path),
                "title": img.title,
                "source_id": img.source_id,
                "aspect_ratio": img.aspect_ratio,
                "width": img.width,
                "height": img.height,
                "primary_color": primary_color,
                "accessible": img.accessible,
                "is_public": img.is_public,
                "source_created_at": img.source_created_at,
                "total_view": img.total_view,
                "total_bookmarks": img.total_bookmarks,
                "total_comments": img.total_comments,
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
                "width": img.width,
                "height": img.height,
                "primary_color": primary_color,
                "author": {
                    "id": auth.id,
                    "name": auth.name,
                    "platform_id": auth.platform_id,
                    "platform": auth.platform,
                },
                "tags": tags_json,
            }));
        }
    }

    Ok(output)
}

/// Get unprocessed images (images that haven't been color-extracted yet).
/// Checks `primary_l IS NULL` as the indicator of unprocessed status.
pub async fn find_unprocessed(db: &DatabaseConnection) -> Result<Vec<image::Model>, DbErr> {
    exclude_deleted(Image::find())
        .filter(image::Column::IsPublic.eq(false))
        .filter(image::Column::PrimaryL.is_null())
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
    if let Some(accessible) = data.get("accessible") {
        if accessible.is_null() {
            active.accessible = Set(None);
        } else if let Some(b) = accessible.as_bool() {
            active.accessible = Set(Some(b));
        }
    }
    if let Some(is_public) = data.get("is_public").and_then(|v| v.as_bool()) {
        active.is_public = Set(is_public);
    }
    if let Some(avatar_available) = data.get("avatar_available") {
        if avatar_available.is_null() {
            active.avatar_available = Set(None);
        } else if let Some(b) = avatar_available.as_bool() {
            active.avatar_available = Set(Some(b));
        }
    }
    if let Some(colors) = data.get("colors") {
        active.colors = Set(Some(colors.clone()));
    }

    let result = active.update(db).await?;
    Ok(Some(result))
}

/// Seed selection method for discover crawling.
#[derive(Debug, Clone, Copy, Default)]
pub enum SeedMethod {
    /// Time-decayed engagement score (views + 3×bookmarks + 2×comments)
    #[default]
    Popularity,
    /// Sort by total_view descending
    Views,
    /// Sort by total_bookmarks descending
    Bookmarks,
    /// Random selection
    Random,
}

impl SeedMethod {
    pub fn from_str(s: &str) -> Self {
        match s {
            "views" => Self::Views,
            "bookmarks" => Self::Bookmarks,
            "random" => Self::Random,
            _ => Self::Popularity,
        }
    }
}

/// Find seed images for discover/next-hop crawling.
/// Returns top `limit` images ranked by the given `method`.
pub async fn find_discover_seeds(
    db: &DatabaseConnection,
    limit: u64,
    method: SeedMethod,
) -> Result<Vec<image::Model>, DbErr> {
    let base_query = || {
        exclude_deleted(Image::find())
            .filter(image::Column::SourceId.is_not_null())
            .filter(image::Column::IsPublic.eq(true))
    };

    match method {
        SeedMethod::Views => Ok(base_query()
            .order_by_desc(image::Column::TotalView)
            .limit(limit)
            .all(db)
            .await?),
        SeedMethod::Bookmarks => Ok(base_query()
            .order_by_desc(image::Column::TotalBookmarks)
            .limit(limit)
            .all(db)
            .await?),
        SeedMethod::Random => {
            // Fetch a larger pool then shuffle in Rust (SQLite RANDOM() not portable via SeaORM)
            let pool = base_query().limit(limit * 5).all(db).await?;
            let mut pool = pool;
            // Fisher-Yates shuffle with a simple seed from timestamp
            let len = pool.len();
            if len <= limit as usize {
                return Ok(pool);
            }
            let seed = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) as usize;
            for i in (1..len).rev() {
                let j = seed.wrapping_mul(6364136223846793005).wrapping_add(i) % (i + 1);
                pool.swap(i, j);
            }
            pool.truncate(limit as usize);
            Ok(pool)
        }
        SeedMethod::Popularity => {
            let candidates = base_query()
                .order_by_desc(image::Column::TotalBookmarks)
                .limit(limit * 3)
                .all(db)
                .await?;

            let mut scored: Vec<(f64, image::Model)> = candidates
                .into_iter()
                .map(|img| (popularity_score(&img), img))
                .collect();
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            Ok(scored
                .into_iter()
                .take(limit as usize)
                .map(|(_, img)| img)
                .collect())
        }
    }
}

/// Count accessible images
pub async fn count_accessible(db: &DatabaseConnection) -> Result<u64, DbErr> {
    exclude_deleted(Image::find())
        .filter(image::Column::Accessible.eq(true))
        .count(db)
        .await
}

/// Create image record
pub async fn create_image(
    db: &DatabaseConnection,
    data: &serde_json::Value,
) -> Result<image::Model, DbErr> {
    // Parse source_created_at from string if present (format: "YYYY-MM-DD HH:MM:SS")
    let source_created_at = data["source_created_at"]
        .as_str()
        .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok())
        .map(|naive| naive.and_utc().fixed_offset());

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
        source_created_at: Set(source_created_at),
        total_view: Set(data["total_view"].as_i64().unwrap_or(0)),
        total_bookmarks: Set(data["total_bookmarks"].as_i64().unwrap_or(0)),
        total_comments: Set(data["total_comments"].as_i64().unwrap_or(0)),
        illust_type: Set(data["illust_type"].as_str().map(|s| s.to_string())),
        x_restrict: Set(data["x_restrict"].as_i64().unwrap_or(0) as i32),
        illust_ai_type: Set(data["illust_ai_type"].as_i64().unwrap_or(0) as i32),
        ..Default::default()
    };
    model.insert(db).await
}

/// Search images by color similarity in LAB space.
///
/// - `lab`: target color as [L, a, b]
/// - `mode`: "primary" (match primary color) or "palette" (match any palette color)
/// - `max_dist`: maximum squared Euclidean distance in LAB space (optional cutoff)
/// - `limit`: max results
///
/// Returns images sorted by ascending distance (most similar first).
pub async fn color_search(
    db: &DatabaseConnection,
    lab: [f64; 3],
    mode: &str,
    max_dist: Option<f64>,
    limit: u64,
    config: &AppConfig,
) -> Result<Vec<serde_json::Value>, DbErr> {
    let target_l = lab[0];
    let target_a = lab[1];
    let target_b = lab[2];

    // Bounding box pre-filter: sqrt(max_dist) gives max per-axis deviation
    let box_radius = max_dist.map(|d| d.sqrt()).unwrap_or(100.0);

    if mode == "primary" {
        // Search by primary color (stored on images table)
        // Pre-filter with bounding box, compute exact distance in Rust
        let results = exclude_deleted(Image::find())
            .filter(image::Column::IsPublic.eq(true))
            .filter(image::Column::PrimaryL.is_not_null())
            .filter(image::Column::PrimaryL.between(target_l - box_radius, target_l + box_radius))
            .filter(image::Column::PrimaryA.between(target_a - box_radius, target_a + box_radius))
            .filter(image::Column::PrimaryB.between(target_b - box_radius, target_b + box_radius))
            .all(db)
            .await?;

        let mut scored: Vec<(f64, image::Model)> = results
            .into_iter()
            .filter_map(|img| {
                let l = img.primary_l?;
                let a = img.primary_a.unwrap_or(0.0);
                let b = img.primary_b.unwrap_or(0.0);
                let dist = (l - target_l).powi(2) + (a - target_a).powi(2) + (b - target_b).powi(2);
                if max_dist.is_none() || dist <= max_dist.unwrap() {
                    Some((dist, img))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit as usize);

        let output = scored
            .into_iter()
            .map(|(dist, img)| {
                serde_json::json!({
                    "id": img.id,
                    "src": format!("{}{}", config.cdn_base_url, img.image_path),
                    "title": img.title,
                    "aspect_ratio": img.aspect_ratio,
                    "width": img.width,
                    "height": img.height,
                    "primary_color": {
                        "rgb": img.colors.as_ref().and_then(|c| c.get("primary_color")).cloned(),
                        "lab": [img.primary_l, img.primary_a, img.primary_b],
                    },
                    "distance": dist,
                })
            })
            .collect();

        Ok(output)
    } else {
        // Search by palette color (match the closest palette entry per image)
        use crate::db::entities::image_color_palette::{self, Entity as PaletteEntity};

        // Bounding box pre-filter on palette table
        let palette_results = PaletteEntity::find()
            .filter(
                image_color_palette::Column::LabL
                    .between(target_l - box_radius, target_l + box_radius),
            )
            .filter(
                image_color_palette::Column::LabA
                    .between(target_a - box_radius, target_a + box_radius),
            )
            .filter(
                image_color_palette::Column::LabB
                    .between(target_b - box_radius, target_b + box_radius),
            )
            .all(db)
            .await?;

        // Group by image_id, keep min distance
        let mut best_by_image: std::collections::HashMap<i32, (f64, &image_color_palette::Model)> =
            std::collections::HashMap::new();
        for p in &palette_results {
            let dist = (p.lab_l - target_l).powi(2)
                + (p.lab_a - target_a).powi(2)
                + (p.lab_b - target_b).powi(2);
            if max_dist.is_some() && dist > max_dist.unwrap() {
                continue;
            }
            let entry = best_by_image.entry(p.image_id).or_insert((dist, p));
            if dist < entry.0 {
                *entry = (dist, p);
            }
        }

        // Sort by distance and take limit
        let mut sorted: Vec<(i32, f64, &image_color_palette::Model)> = best_by_image
            .into_iter()
            .map(|(img_id, (dist, p))| (img_id, dist, p))
            .collect();
        sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(limit as usize);

        // Fetch corresponding images
        let image_ids: Vec<i32> = sorted.iter().map(|(id, _, _)| *id).collect();
        let images: Vec<image::Model> = if image_ids.is_empty() {
            Vec::new()
        } else {
            exclude_deleted(Image::find())
                .filter(image::Column::Id.is_in(image_ids))
                .all(db)
                .await?
        };
        let image_map: std::collections::HashMap<i32, &image::Model> =
            images.iter().map(|i| (i.id, i)).collect();

        let mut output = Vec::new();
        for (img_id, dist, matched_color) in sorted {
            if let Some(img) = image_map.get(&img_id) {
                output.push(serde_json::json!({
                    "id": img.id,
                    "src": format!("{}{}", config.cdn_base_url, img.image_path),
                    "title": img.title,
                    "aspect_ratio": img.aspect_ratio,
                    "width": img.width,
                    "height": img.height,
                    "primary_color": {
                        "rgb": img.colors.as_ref().and_then(|c| c.get("primary_color")).cloned(),
                        "lab": [img.primary_l, img.primary_a, img.primary_b],
                    },
                    "matched_color": {
                        "rgb": [matched_color.rgb_r, matched_color.rgb_g, matched_color.rgb_b],
                        "lab": [matched_color.lab_l, matched_color.lab_a, matched_color.lab_b],
                    },
                    "distance": dist,
                }));
            }
        }

        Ok(output)
    }
}
