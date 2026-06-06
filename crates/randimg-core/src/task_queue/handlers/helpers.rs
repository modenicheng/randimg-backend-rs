use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::WorkerState;
use crate::db::query;

use super::super::jobs::*;

// ---------------------------------------------------------------------------
// Shared helpers used by crawl, download, and discover handlers
// ---------------------------------------------------------------------------

/// Info needed to submit a download job after saving an illust.
pub(super) struct DownloadInfo {
    pub(super) image_id: i32,
    pub(super) source_image_url: String,
    pub(super) image_path: String,
}

/// Save an Illust to the database. Returns download infos for newly created images.
pub(super) async fn save_illust(
    state: &WorkerState,
    illust: &crate::pixiv::Illust,
    illust_type_filter: Option<Vec<String>>,
    exclude_r18: Option<bool>,
    exclude_ai: Option<bool>,
) -> Result<Vec<DownloadInfo>, String> {
    // Filter by illust type if filter is specified
    if let Some(ref types) = illust_type_filter {
        if !types.is_empty() {
            let illust_type = illust.r#type.as_ref()
                .map(|t| format!("{:?}", t).to_lowercase())
                .unwrap_or_default();
            if !types.contains(&illust_type) {
                tracing::debug!("Skipping illust {} (type={}, not in filter {:?})", illust.id, illust_type, types);
                return Ok(Vec::new());
            }
        }
    }

    // Filter out R18 content if requested
    if exclude_r18.unwrap_or(false) {
        if illust.x_restrict.unwrap_or(0) > 0 {
            tracing::debug!("Skipping R18 illust {} (x_restrict={})", illust.id, illust.x_restrict.unwrap_or(0));
            return Ok(Vec::new());
        }
    }

    // Filter out AI-generated content if requested
    if exclude_ai.unwrap_or(false) {
        if illust.illust_ai_type.unwrap_or(0) >= 2 {
            tracing::debug!("Skipping AI illust {} (illust_ai_type={})", illust.id, illust.illust_ai_type.unwrap_or(0));
            return Ok(Vec::new());
        }
    }

    // Filter out deleted/unviewable images (Pixiv marks these with visible=false)
    if illust.visible == Some(false) {
        tracing::debug!("Skipping unviewable illust {} (visible=false)", illust.id);
        return Ok(Vec::new());
    }

    // Filter out placeholder images (deleted, restricted, sanity-level blocked)
    if let Some((first_url, _)) = get_image_pages(illust).first() {
        if first_url.contains("limit_unknown")
            || first_url.contains("limit_unviewable")
            || first_url.contains("limit_sanity_level")
        {
            tracing::debug!(
                "Skipping placeholder illust {} (url={})",
                illust.id,
                first_url
            );
            return Ok(Vec::new());
        }
    }

    let db = &state.db;
    let mut downloads = Vec::new();

    let user = illust.user.as_ref();
    let author_name = user.and_then(|u| u.name.as_deref()).unwrap_or("unknown");
    let author_id_str = user.and_then(|u| u.id.map(|id| id.to_string()));

    let author =
        query::author::find_or_create(db, author_name, Some("pixiv"), author_id_str.as_deref())
            .await
            .map_err(|e| format!("Failed to upsert author: {}", e))?;

    let pages = get_image_pages(illust);

    for (page_idx, (image_url, _)) in pages.iter().enumerate() {
        let ext = if image_url.contains(".png") {
            "png"
        } else {
            "jpg"
        };
        let image_path = format!("{}_p{}.{}", illust.id, page_idx, ext);

        // ┌─────────────────────────────────────────────────────────────────────┐
        // │ 尺寸策略：占位写入 + 下载后校验                                      │
        // │                                                                     │
        // │ 为什么：Pixiv 移动端 API（v2/illust/detail）的 meta_pages 仅返回     │
        // │ 每页图片的 URL，不返回各页独立的 width/height；illust 级别的          │
        // │ width × height 仅代表第一页尺寸，对多页作品后续页不适用。             │
        // │                                                                     │
        // │ 做法：此处将 width / height / aspect_ratio 写为 0，作为占位符；       │
        // │ 实际像素尺寸由 handle_download() 在下载完成后读取文件并 UPDATE。      │
        // │                                                                     │
        // │ 边界：在 save_illust 到 handle_download 之间，数据库中这些字段为 0。  │
        // │ 这是可接受的——此时图片尚未下载，不可供服务端使用。                    │
        // └─────────────────────────────────────────────────────────────────────┘
        let width: i32 = 0;
        let height: i32 = 0;
        let aspect_ratio: f32 = 0.0;

        let existing = crate::db::entities::image::Entity::find()
            .filter(crate::db::entities::image::Column::ImagePath.eq(&image_path))
            .one(db)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(existing) = existing {
            // Update engagement metrics for existing images
            let new_view = illust.total_view.unwrap_or(0) as i64;
            let new_bookmarks = illust.total_bookmarks.unwrap_or(0) as i64;
            let new_comments = illust.total_comments.unwrap_or(0) as i64;
            if new_view != existing.total_view
                || new_bookmarks != existing.total_bookmarks
                || new_comments != existing.total_comments
            {
                let mut active: crate::db::entities::image::ActiveModel = existing.into();
                active.total_view = sea_orm::Set(new_view);
                active.total_bookmarks = sea_orm::Set(new_bookmarks);
                active.total_comments = sea_orm::Set(new_comments);
                let _ = active.update(db).await;
            }
            continue;
        }

        let image_data = serde_json::json!({
            "title": illust.title,
            "image_path": image_path,
            "source_url": format!("https://www.pixiv.net/artworks/{}", illust.id),
            "source_id": illust.id as i64,
            "source_image_url": image_url,
            "author_id": author.id,
            "width": width,
            "height": height,
            "aspect_ratio": aspect_ratio,
            "source_created_at": illust.create_date
                .map(|d| d.naive_utc().format("%Y-%m-%d %H:%M:%S").to_string()),
            "total_view": illust.total_view.unwrap_or(0),
            "total_bookmarks": illust.total_bookmarks.unwrap_or(0),
            "total_comments": illust.total_comments.unwrap_or(0),
            "illust_type": illust.r#type.as_ref().map(|t| format!("{:?}", t).to_lowercase()),
            "x_restrict": illust.x_restrict.unwrap_or(0),
            "illust_ai_type": illust.illust_ai_type.unwrap_or(0),
        });

        let image = query::image::create_image(db, &image_data)
            .await
            .map_err(|e| format!("Failed to create image: {}", e))?;

        if let Some(tags) = &illust.tags {
            for tag in tags {
                let tag_model =
                    query::tag::find_or_create(db, &tag.name, tag.translated_name.as_deref())
                        .await
                        .map_err(|e| format!("Failed to upsert tag: {}", e))?;

                let result = crate::db::entities::image_tag_association::ActiveModel {
                    image_id: sea_orm::Set(image.id),
                    tag_id: sea_orm::Set(tag_model.id),
                }
                .insert(db)
                .await;
                match result {
                    Ok(_) => {}
                    Err(sea_orm::DbErr::RecordNotInserted) => {}
                    Err(e) => return Err(format!("Failed to insert tag association: {}", e)),
                }
            }
        }

        downloads.push(DownloadInfo {
            image_id: image.id,
            source_image_url: image_url.clone(),
            image_path: image_path.clone(),
        });
    }

    tracing::info!("Saved illust {} with {} pages", illust.id, pages.len());
    Ok(downloads)
}

pub(super) fn get_image_pages(illust: &crate::pixiv::Illust) -> Vec<(String, String)> {
    let mut pages = Vec::new();

    if let Some(meta_pages) = &illust.meta_pages {
        for (i, page) in meta_pages.iter().enumerate() {
            if let Some(urls) = &page.image_urls {
                let url = urls
                    .original
                    .as_ref()
                    .or(urls.large.as_ref())
                    .or(urls.medium.as_ref());
                if let Some(url) = url {
                    pages.push((url.clone(), format!("{}_p{}", illust.id, i)));
                }
            }
        }
    }

    if pages.is_empty() {
        let url = illust
            .meta_single_page
            .as_ref()
            .and_then(|msp| msp.original_image_url.as_ref())
            .or_else(|| {
                illust
                    .image_urls
                    .as_ref()
                    .and_then(|iu| iu.original.as_ref())
            })
            .or_else(|| illust.image_urls.as_ref().and_then(|iu| iu.large.as_ref()));

        if let Some(url) = url {
            pages.push((url.clone(), format!("{}_p0", illust.id)));
        }
    }

    pages
}

pub(super) fn extract_param_from_url(url: &str, param: &str) -> Option<String> {
    url.split('?')
        .nth(1)?
        .split('&')
        .find(|p| p.starts_with(&format!("{}=", param)))
        .and_then(|p| p.split('=').nth(1))
        .map(|v| v.to_string())
}

/// Mark an image as downloaded in the database.
pub(super) async fn mark_downloaded(db: &sea_orm::DatabaseConnection, image_id: i32) -> Result<(), sea_orm::DbErr> {
    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img) = Image::find_by_id(image_id).one(db).await? {
        let mut active: image::ActiveModel = img.into();
        active.downloaded = Set(true);
        active.update(db).await?;
    }
    Ok(())
}

/// Spawn downstream pipeline tasks (color extract, upload, accessibility check)
/// as children of the download task (not the root crawl job).
pub(super) async fn spawn_downstream_children(
    job: &DownloadJob,
    current_id: &str,
    state: &WorkerState,
) -> Result<(), String> {
    let upstream_id = job.root_job_id.as_deref().unwrap_or(current_id);

    let color_task_id = uuid::Uuid::new_v4().to_string();
    let color_job = ColorExtractJob {
        image_id: job.image_id,
        image_path: job.image_path.clone(),
        parent_job_id: Some(current_id.to_string()),
        task_id: Some(color_task_id.clone()),
        max_retries: 0,  // CPU-bound, no retry
        backoff_base: state.config.task_backoff_base,
    };
    let color_metadata = serde_json::to_value(&color_job).unwrap();

    let upload_task_id = uuid::Uuid::new_v4().to_string();
    let upload_job = UploadJob {
        image_id: job.image_id,
        image_path: job.image_path.clone(),
        parent_job_id: Some(current_id.to_string()),
        task_id: Some(upload_task_id.clone()),
        max_retries: state.config.task_max_retries,
        backoff_base: state.config.task_backoff_base,
    };
    let upload_metadata = serde_json::to_value(&upload_job).unwrap();

    let a11y_task_id = uuid::Uuid::new_v4().to_string();
    let a11y_job = AccessibilityCheckJob {
        image_id: job.image_id,
        image_path: job.image_path.clone(),
        parent_job_id: Some(current_id.to_string()),
        task_id: Some(a11y_task_id.clone()),
        max_retries: state.config.task_max_retries,
        backoff_base: state.config.task_backoff_base,
    };
    let a11y_metadata = serde_json::to_value(&a11y_job).unwrap();

    let color_fut = state.queue_backend.push_task(
        &color_job, "color_extract", color_metadata, &state.db, Some(current_id), Some(upstream_id), None, Some(job.image_id), Some(&color_task_id),
    );
    let upload_fut = state.queue_backend.push_task(
        &upload_job, "upload", upload_metadata, &state.db, Some(current_id), Some(upstream_id), None, Some(job.image_id), Some(&upload_task_id),
    );
    let a11y_fut = state.queue_backend.push_task(
        &a11y_job, "accessibility_check", a11y_metadata, &state.db, Some(current_id), Some(upstream_id), None, Some(job.image_id), Some(&a11y_task_id),
    );

    let (color_res, upload_res, a11y_res) = tokio::join!(color_fut, upload_fut, a11y_fut);

    color_res.map_err(|e| format!("Failed to submit color_extract task: {}", e))?;
    upload_res.map_err(|e| format!("Failed to submit upload task: {}", e))?;
    a11y_res.map_err(|e| format!("Failed to submit accessibility_check task: {}", e))?;
    Ok(())
}
