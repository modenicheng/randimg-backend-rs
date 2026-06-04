use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};

use crate::WorkerState;
use crate::db::query;
use crate::db::query::image::SeedMethod;
use crate::pixiv::PixivApi;

use super::jobs::*;

// ---------------------------------------------------------------------------
// Job handlers
// ---------------------------------------------------------------------------

/// Crawl Pixiv illustrations (by user, ranking, or bookmarks).
pub async fn handle_crawl(job: CrawlJob, state: &Arc<WorkerState>) -> Result<(), String> {
    let current_id = job.task_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let crawl_type = job.crawl_type;
    let crawler_id = job.crawler_id;

    // Mark crawler as running
    query::crawler::mark_running(&state.db, crawler_id)
        .await
        .map_err(|e| format!("Failed to mark crawler running: {}", e))?;

    // Authenticate Pixiv API: reuse stored token if valid, otherwise refresh
    let api = crate::pixiv::create_api(&state.config.pixiv_proxy, &state.config.pixiv_accept_lang).await;
    let credential_id = if let Some(cred) = query::pixiv_credential::find_one_active_random(&state.db)
        .await
        .map_err(|e| format!("Failed to fetch credential: {}", e))?
    {
        crate::pixiv::auth_with_credential(&api, &cred, &state.db).await?
    } else {
        return Err("No active Pixiv credentials found".into());
    };

    let result = match crawl_type {
        0 => crawl_ranking(&**state, &api, credential_id, &job, &current_id).await,
        1 => crawl_user(&**state, &api, credential_id, &job, &current_id).await,
        2 => crawl_bookmarks(&**state, &api, credential_id, &job, &current_id).await,
        _ => Err(format!("Unknown crawl_type: {}", crawl_type)),
    };

    // Update crawler status based on result
    match &result {
        Ok(()) => {
            let total = crate::db::entities::image::Entity::find()
                .filter(crate::db::entities::image::Column::SourceId.is_not_null())
                .count(&state.db)
                .await
                .unwrap_or(0) as i32;

            let _ = query::crawler::mark_completed(&state.db, crawler_id, total).await;

            // Trigger autonomous discover for next-hop crawling (unless disabled)
            if job.disable_discover.unwrap_or(false) {
                tracing::info!(crawler_id, "Discover disabled for this crawl job, skipping");
            } else {
            let discover_task_id = uuid::Uuid::new_v4().to_string();
            let discover_job = DiscoverJob {
                hop: 0,
                max_hops: job.discover_hops,
                seed_limit: job.discover_seed_limit,
                seed_method: job.discover_seed_method.clone(),
                credential_ids: job.credential_ids.clone(),
                parent_job_id: Some(current_id.clone()),
                task_id: Some(discover_task_id.clone()),
                root_job_id: Some(current_id.clone()),
                max_retries: state.config.task_max_retries,
                backoff_base: state.config.task_backoff_base,
            };
            let metadata = serde_json::to_value(&discover_job)
                .map_err(|e| format!("Failed to serialize discover job: {}", e))?;
            state
                .queue_backend
                .push_task(&discover_job, "discover", metadata, &state.db, Some(&current_id), Some(&current_id), None, None, Some(&discover_task_id))
                .await
                .map_err(|e| format!("Failed to submit discover task: {}", e))?;
            } // end discover check
        }
        Err(_) => {
            let _ = query::crawler::mark_failed(&state.db, crawler_id).await;
        }
    }

    result
}

async fn crawl_ranking(
    state: &WorkerState,
    api: &PixivApi,
    credential_id: i32,
    job: &CrawlJob,
    parent_id: &str,
) -> Result<(), String> {
    let start_date = job.target_start_date.as_deref();
    let date = start_date.map(|s| if s.len() >= 10 { &s[..10] } else { s });
    let mode = job.ranking_mode.as_deref().unwrap_or("day");
    let max_pages = job.max_pages.unwrap_or(0);

    let mut offset = 0u32;
    let mut pages_processed = 0u32;
    loop {
        let resp = match api
            .illust_ranking(Some(mode), date, Some(offset))
            .await
        {
            Ok(r) => r,
            Err(e) if e.is_auth_error() => {
                crate::pixiv::recover_auth(api, credential_id, &state.db).await?;
                api.illust_ranking(Some(mode), date, Some(offset))
                    .await
                    .map_err(|e| format!("Ranking API error after auth recovery: {}", e))?
            }
            Err(e) => return Err(format!("Ranking API error: {}", e)),
        };

        let data = resp.data.ok_or("No data in ranking response")?;
        let illusts = data.illusts;

        if illusts.is_empty() {
            break;
        }

        for illust in &illusts {
            let downloads = save_illust(state, illust, job.illust_type_filter.clone(), job.exclude_r18, job.exclude_ai).await?;
            for dl in downloads {
                let download_task_id = uuid::Uuid::new_v4().to_string();
                let download_job = DownloadJob {
                    image_id: dl.image_id,
                    source_image_url: dl.source_image_url,
                    image_path: dl.image_path,
                    parent_job_id: Some(parent_id.to_string()),
                    root_job_id: Some(parent_id.to_string()),
                    task_id: Some(download_task_id.clone()),
                    max_retries: state.config.task_max_retries,
                    backoff_base: state.config.task_backoff_base,
                };
                let metadata = serde_json::to_value(&download_job)
                    .map_err(|e| format!("Failed to serialize download job: {}", e))?;
                state
                    .queue_backend
                    .push_task(&download_job, "download", metadata, &state.db, Some(parent_id), Some(parent_id), None, Some(dl.image_id), Some(&download_task_id))
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
            }
        }

        pages_processed += 1;
        if max_pages > 0 && pages_processed >= max_pages {
            tracing::info!(max_pages, "Reached page limit for ranking crawl");
            break;
        }

        if data.next_url.is_none() {
            break;
        }
        offset += illusts.len() as u32;
    }

    Ok(())
}

async fn crawl_user(
    state: &WorkerState,
    api: &PixivApi,
    credential_id: i32,
    job: &CrawlJob,
    parent_id: &str,
) -> Result<(), String> {
    let user_id = job
        .target_user_id
        .as_deref()
        .ok_or("missing target_user_id")?
        .parse::<u64>()
        .map_err(|_| "invalid target_user_id")?;

    let illust_type = job.illust_type.as_deref().unwrap_or("illust");
    let max_pages = job.max_pages.unwrap_or(0);

    let mut offset = 0u32;
    let mut pages_processed = 0u32;
    loop {
        let resp = match api
            .user_illusts(user_id, Some(illust_type), Some(offset))
            .await
        {
            Ok(r) => r,
            Err(e) if e.is_auth_error() => {
                crate::pixiv::recover_auth(api, credential_id, &state.db).await?;
                api.user_illusts(user_id, Some(illust_type), Some(offset))
                    .await
                    .map_err(|e| format!("User illusts API error after auth recovery: {}", e))?
            }
            Err(e) => return Err(format!("User illusts API error: {}", e)),
        };

        let data = resp.data.ok_or("No data in user_illusts response")?;
        let illusts = data.illusts;

        if illusts.is_empty() {
            break;
        }

        for illust in &illusts {
            let downloads = save_illust(state, illust, job.illust_type_filter.clone(), job.exclude_r18, job.exclude_ai).await?;
            for dl in downloads {
                let download_task_id = uuid::Uuid::new_v4().to_string();
                let download_job = DownloadJob {
                    image_id: dl.image_id,
                    source_image_url: dl.source_image_url,
                    image_path: dl.image_path,
                    parent_job_id: Some(parent_id.to_string()),
                    root_job_id: Some(parent_id.to_string()),
                    task_id: Some(download_task_id.clone()),
                    max_retries: state.config.task_max_retries,
                    backoff_base: state.config.task_backoff_base,
                };
                let metadata = serde_json::to_value(&download_job)
                    .map_err(|e| format!("Failed to serialize download job: {}", e))?;
                state
                    .queue_backend
                    .push_task(&download_job, "download", metadata, &state.db, Some(parent_id), Some(parent_id), None, Some(dl.image_id), Some(&download_task_id))
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
            }
        }

        pages_processed += 1;
        if max_pages > 0 && pages_processed >= max_pages {
            tracing::info!(max_pages, user_id, "Reached page limit for user crawl");
            break;
        }

        if data.next_url.is_none() {
            break;
        }
        offset += illusts.len() as u32;
    }

    Ok(())
}

async fn crawl_bookmarks(
    state: &WorkerState,
    api: &PixivApi,
    credential_id: i32,
    job: &CrawlJob,
    parent_id: &str,
) -> Result<(), String> {
    let user_id = api
        .user_id()
        .await
        .ok_or("Not authenticated or no user_id")?;

    let tags = job.target_search_prompt.as_deref().filter(|s| !s.is_empty());
    let max_pages = job.max_pages.unwrap_or(0);

    tracing::info!(
        user_id,
        tags,
        max_pages,
        credential_id,
        "Starting bookmarks crawl"
    );

    let mut max_bookmark_id: Option<u64> = None;
    let mut pages_processed = 0u32;
    loop {
        let resp = match api
            .user_bookmarks_illust(user_id, Some("public"), max_bookmark_id, tags)
            .await
        {
            Ok(r) => r,
            Err(e) if e.is_auth_error() => {
                crate::pixiv::recover_auth(api, credential_id, &state.db).await?;
                api.user_bookmarks_illust(user_id, Some("public"), max_bookmark_id, tags)
                    .await
                    .map_err(|e| format!("Bookmarks API error after auth recovery: {}", e))?
            }
            Err(e) => return Err(format!("Bookmarks API error: {}", e)),
        };

        let data = resp.data.ok_or("No data in bookmarks response")?;
        let illusts = data.illusts;

        if illusts.is_empty() {
            break;
        }

        for illust in &illusts {
            let downloads = save_illust(state, illust, job.illust_type_filter.clone(), job.exclude_r18, job.exclude_ai).await?;
            for dl in downloads {
                let download_task_id = uuid::Uuid::new_v4().to_string();
                let download_job = DownloadJob {
                    image_id: dl.image_id,
                    source_image_url: dl.source_image_url,
                    image_path: dl.image_path,
                    parent_job_id: Some(parent_id.to_string()),
                    root_job_id: Some(parent_id.to_string()),
                    task_id: Some(download_task_id.clone()),
                    max_retries: state.config.task_max_retries,
                    backoff_base: state.config.task_backoff_base,
                };
                let metadata = serde_json::to_value(&download_job)
                    .map_err(|e| format!("Failed to serialize download job: {}", e))?;
                state
                    .queue_backend
                    .push_task(&download_job, "download", metadata, &state.db, Some(parent_id), Some(parent_id), None, Some(dl.image_id), Some(&download_task_id))
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
            }
        }

        pages_processed += 1;
        if max_pages > 0 && pages_processed >= max_pages {
            tracing::info!(max_pages, "Reached page limit for bookmarks crawl");
            break;
        }

        if let Some(next_url) = &data.next_url {
            if let Some(id_str) = extract_param_from_url(next_url, "max_bookmark_id") {
                max_bookmark_id = id_str.parse().ok();
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Ok(())
}

/// Info needed to submit a download job after saving an illust.
struct DownloadInfo {
    image_id: i32,
    source_image_url: String,
    image_path: String,
}

/// Save an Illust to the database. Returns download infos for newly created images.
async fn save_illust(
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
        if illust.illust_ai_type.unwrap_or(0) > 0 {
            tracing::debug!("Skipping AI illust {} (illust_ai_type={})", illust.id, illust.illust_ai_type.unwrap_or(0));
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

fn get_image_pages(illust: &crate::pixiv::Illust) -> Vec<(String, String)> {
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

fn extract_param_from_url(url: &str, param: &str) -> Option<String> {
    url.split('?')
        .nth(1)?
        .split('&')
        .find(|p| p.starts_with(&format!("{}=", param)))
        .and_then(|p| p.split('=').nth(1))
        .map(|v| v.to_string())
}

/// Mark an image as downloaded in the database.
async fn mark_downloaded(db: &sea_orm::DatabaseConnection, image_id: i32) -> Result<(), sea_orm::DbErr> {
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
async fn spawn_downstream_children(
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

/// Download a single image from Pixiv to local disk.
pub async fn handle_download(job: DownloadJob, state: &Arc<WorkerState>) -> Result<(), String> {
    let current_id = job.task_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let file_path = format!("{}/{}", state.config.image_dir, job.image_path);

    // 1. Check if file already exists on disk
    if tokio::fs::metadata(&file_path).await.is_ok() {
        tracing::info!("File already exists on disk, skipping download: {}", job.image_path);
        if let Err(e) = mark_downloaded(&state.db, job.image_id).await {
            tracing::warn!(image_id = job.image_id, "Failed to mark as downloaded: {}", e);
        }

        // ┌─────────────────────────────────────────────────────────────────────┐
        // │ 校验已存在文件：读取实际像素尺寸并回写数据库                          │
        // │                                                                     │
        // │ 为什么：save_illust() 因 Pixiv API 不返回各页独立尺寸，只能写入 0。  │
        // │ 此处对已存在文件补全真实宽高，确保数据库数据准确。                    │
        // │                                                                     │
        // │ 错误处理：若文件损坏无法解码，删除文件并返回错误，触发 Apalis 重试。  │
        // └─────────────────────────────────────────────────────────────────────┘
        {
            use crate::db::entities::image::{self, Entity as Image};
            let fp = file_path.clone();
            let image_id = job.image_id;

            let dimensions = match tokio::task::spawn_blocking(move || {
                let img = ::image::open(&fp)
                    .map_err(|e| format!("Failed to open/validate image: {}", e))?;
                let (w, h) = (img.width() as i32, img.height() as i32);
                Ok::<_, String>((w, h))
            })
            .await
            .map_err(|e| format!("spawn_blocking panicked: {}", e))?
            {
                Ok(dims) => dims,
                Err(e) => {
                    tracing::error!(image_id = job.image_id, "Image file corrupt/invalid: {}", e);
                    let _ = tokio::fs::remove_file(&file_path).await;
                    return Err(e.into());
                }
            };

            let (w, h) = dimensions;
            let ar = if h > 0 { w as f32 / h as f32 } else { 0.0 };

            if let Some(img_model) = Image::find_by_id(image_id)
                .one(&state.db)
                .await
                .map_err(|e| e.to_string())?
            {
                let mut active: image::ActiveModel = img_model.into();
                active.width = Set(w);
                active.height = Set(h);
                active.aspect_ratio = Set(ar);
                active.update(&state.db).await.map_err(|e| e.to_string())?;
            }
        }

        spawn_downstream_children(&job, &current_id, &**state).await?;
        return Ok(());
    }

    // 2. Check DB downloaded flag — if true but file is missing, log warning and re-download
    {
        use crate::db::entities::image::Entity as Image;
        if let Some(img) = Image::find_by_id(job.image_id).one(&state.db).await.map_err(|e| e.to_string())? {
            if img.downloaded {
                tracing::warn!(
                    image_id = job.image_id,
                    "DB says downloaded but file missing, re-downloading: {}",
                    job.image_path
                );
            }
        }
    }

    // 3. Perform the actual download using crate's built-in downloader.
    //    DownloadManager auto-configures headers (Referer) and handles file I/O.
    //    Retries are managed by Apalis (RetryPolicy), not the downloader.
    {
        use std::path::Path;
        let img_path = Path::new(&job.image_path);
        let parent = img_path.parent().unwrap_or(Path::new(""));
        let filename = img_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Invalid image_path: {}", job.image_path))?;

        // The crate only creates the base output_dir; ensure subdirectories exist.
        let output_dir = if parent.as_os_str().is_empty() {
            Path::new(&state.config.image_dir).to_path_buf()
        } else {
            let dir = Path::new(&state.config.image_dir).join(parent);
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(|e| format!("Failed to create directory: {}", e))?;
            dir
        };

        let dm = crate::pixiv::downloader::DownloadManager::new(
            state.http_client.clone(),
            &output_dir,
        );
        dm.download(&job.source_image_url, filename)
            .await
            .map_err(|e| format!("Download failed: {}", e))?;
    }

    tracing::info!("Downloaded image {} to {}", job.image_id, file_path);

    // 4. Mark as downloaded in DB
    if let Err(e) = mark_downloaded(&state.db, job.image_id).await {
        tracing::warn!(image_id = job.image_id, "Failed to mark as downloaded: {}", e);
    }

    // ┌─────────────────────────────────────────────────────────────────────┐
    // │ 校验下载文件：读取实际像素尺寸并回写数据库                            │
    // │                                                                     │
    // │ 为什么：save_illust() 因 Pixiv API 不返回各页独立尺寸，只能写入 0。  │
    // │ 此处在下载完成后解码图片，获取真实宽高并 UPDATE 数据库。              │
    // │                                                                     │
    // │ 错误处理：若文件损坏无法解码，删除文件并返回错误，触发 Apalis 重试。  │
    // └─────────────────────────────────────────────────────────────────────┘
    {
        use crate::db::entities::image::{self, Entity as Image};
        let fp = file_path.clone();
        let image_id = job.image_id;

        let dimensions = match tokio::task::spawn_blocking(move || {
            let img = ::image::open(&fp)
                .map_err(|e| format!("Failed to open/validate image: {}", e))?;
            let (w, h) = (img.width() as i32, img.height() as i32);
            Ok::<_, String>((w, h))
        })
        .await
        .map_err(|e| format!("spawn_blocking panicked: {}", e))?
        {
            Ok(dims) => dims,
            Err(e) => {
                tracing::error!(image_id = job.image_id, "Image file corrupt/invalid: {}", e);
                let _ = tokio::fs::remove_file(&file_path).await;
                return Err(e.into());
            }
        };

        let (w, h) = dimensions;
        let ar = if h > 0 { w as f32 / h as f32 } else { 0.0 };

        if let Some(img_model) = Image::find_by_id(image_id)
            .one(&state.db)
            .await
            .map_err(|e| e.to_string())?
        {
            let mut active: image::ActiveModel = img_model.into();
            active.width = Set(w);
            active.height = Set(h);
            active.aspect_ratio = Set(ar);
            active.update(&state.db).await.map_err(|e| e.to_string())?;
        }
    }

    // 5. Spawn downstream pipeline tasks
    spawn_downstream_children(&job, &current_id, &**state).await?;

    Ok(())
}

/// Extract color palette from a downloaded image.
///
/// The heavy computation (image decode + KMeans) is offloaded to
/// `spawn_blocking` so it runs on tokio's blocking thread pool,
/// not on the async worker threads. Combined with the dedicated
/// rayon pool inside `extract_theme_colors`, this ensures color
/// extraction never blocks the async runtime.
pub async fn handle_color_extract(job: ColorExtractJob, state: &Arc<WorkerState>) -> Result<(), String> {
    let image_dir = state.config.image_dir.clone();
    let image_id = job.image_id;

    // CPU-heavy work: image decode + KMeans on blocking thread pool
    let colors = tokio::task::spawn_blocking(move || {
        let full_path = format!("{}/{}", image_dir, job.image_path);
        let img = ::image::open(&full_path)
            .map_err(|e| format!("Failed to open image: {}", e))?;
        Ok::<_, String>(crate::color::extract_theme_colors(&img))
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    // DB writes stay on the async runtime (they are I/O-bound)
    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img_model) = Image::find_by_id(image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        let mut active: image::ActiveModel = img_model.into();
        active.colors = Set(Some(serde_json::to_value(&colors).unwrap()));
        active.primary_l = Set(Some(colors.primary_lab[0]));
        active.primary_a = Set(Some(colors.primary_lab[1]));
        active.primary_b = Set(Some(colors.primary_lab[2]));
        active.update(&state.db).await.map_err(|e| e.to_string())?;
    }

    // Upsert palette entries
    use crate::db::entities::image_color_palette::{self, Entity as PaletteEntity};

    PaletteEntity::delete_many()
        .filter(image_color_palette::Column::ImageId.eq(image_id))
        .exec(&state.db)
        .await
        .map_err(|e| format!("Failed to clear old palette: {}", e))?;

    for (i, (rgb, lab)) in colors
        .colors
        .iter()
        .zip(colors.colors_lab.iter())
        .enumerate()
    {
        let entry = image_color_palette::ActiveModel {
            id: sea_orm::NotSet,
            image_id: Set(image_id),
            color_index: Set(i as i32),
            rgb_r: Set(rgb[0] as i32),
            rgb_g: Set(rgb[1] as i32),
            rgb_b: Set(rgb[2] as i32),
            lab_l: Set(lab[0]),
            lab_a: Set(lab[1]),
            lab_b: Set(lab[2]),
        };
        entry
            .insert(&state.db)
            .await
            .map_err(|e| format!("Failed to insert palette entry: {}", e))?;
    }

    Ok(())
}

/// Upload a downloaded image to DogeCloud OSS.
pub async fn handle_upload(job: UploadJob, state: &Arc<WorkerState>) -> Result<(), String> {
    let file_path = format!("{}/{}", state.config.image_dir, job.image_path);
    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|e| format!("Failed to read image file {}: {}", file_path, e))?;

    state
        .oss
        .upload(&job.image_path, bytes)
        .await
        .map_err(|e| format!("OSS upload failed: {}", e))?;

    tracing::info!(
        image_id = job.image_id,
        path = %job.image_path,
        "Uploaded image to DogeCloud OSS"
    );

    // Mark image as public
    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img_model) = Image::find_by_id(job.image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        let mut active: image::ActiveModel = img_model.into();
        active.is_public = Set(true);
        active.update(&state.db).await.map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Check image accessibility (stub — marks image as accessible).
pub async fn handle_accessibility_check(
    job: AccessibilityCheckJob,
    state: &Arc<WorkerState>,
) -> Result<(), String> {
    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img_model) = Image::find_by_id(job.image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        let mut active: image::ActiveModel = img_model.into();
        active.accessible = Set(Some(true));
        active.update(&state.db).await.map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Discover related illustrations via Pixiv related-illust API.
pub async fn handle_discover(job: DiscoverJob, state: &Arc<WorkerState>) -> Result<(), String> {
    let current_id = job.task_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let hop = job.hop;
    let max_hops = job.max_hops.unwrap_or(state.config.max_discover_hops);
    let seed_limit = job.seed_limit.unwrap_or(state.config.discover_seed_limit);
    let seed_method = job
        .seed_method
        .as_deref()
        .map(SeedMethod::from_str)
        .unwrap_or_default();

    tracing::info!(
        hop,
        max_hops,
        seed_limit,
        ?seed_method,
        "Starting discover task"
    );

    let seeds = query::image::find_discover_seeds(&state.db, seed_limit, seed_method)
        .await
        .map_err(|e| format!("Failed to find discover seeds: {}", e))?;

    if seeds.is_empty() {
        tracing::info!("No discover seeds found, skipping");
        return Ok(());
    }

    tracing::info!(count = seeds.len(), "Selected discover seeds");

    // Authenticate Pixiv API: reuse stored token if valid, otherwise refresh
    let api = crate::pixiv::create_api(&state.config.pixiv_proxy, &state.config.pixiv_accept_lang).await;
    let credential_id = if let Some(ref ids) = job.credential_ids {
        if !ids.is_empty() {
            let creds = query::pixiv_credential::find_active_by_ids(&state.db, ids)
                .await
                .map_err(|e| format!("Failed to fetch credentials by IDs: {}", e))?;
            if creds.is_empty() {
                return Err(format!(
                    "No active credentials found among specified IDs: {:?}", ids
                ));
            }
            let cred = creds.into_iter().next().unwrap();
            crate::pixiv::auth_with_credential(&api, &cred, &state.db).await?
        } else {
            let cred = query::pixiv_credential::find_one_active_random(&state.db)
                .await
                .map_err(|e| format!("Failed to fetch credential: {}", e))?
                .ok_or("No active Pixiv credentials found")?;
            crate::pixiv::auth_with_credential(&api, &cred, &state.db).await?
        }
    } else {
        let cred = query::pixiv_credential::find_one_active_random(&state.db)
            .await
            .map_err(|e| format!("Failed to fetch credential: {}", e))?
            .ok_or("No active Pixiv credentials found")?;
        crate::pixiv::auth_with_credential(&api, &cred, &state.db).await?
    };

    let mut total_discovered = 0u32;
    for seed in &seeds {
        let source_id = seed
            .source_id
            .ok_or_else(|| format!("Seed {} missing source_id", seed.id))?;

        let resp = match api.illust_related(source_id as u64).await {
            Ok(r) => r,
            Err(e) if e.is_auth_error() => {
                crate::pixiv::recover_auth(&api, credential_id, &state.db).await?;
                api.illust_related(source_id as u64)
                    .await
                    .map_err(|e| format!("illust_related API error after auth recovery: {}", e))?
            }
            Err(e) => return Err(format!("illust_related API error: {}", e).into()),
        };

        let data = resp.data.ok_or("No data in illust_related response")?;

        for illust in &data.illusts {
            let downloads = save_illust(state, illust, None, None, None).await?;
            for dl in downloads {
                let download_task_id = uuid::Uuid::new_v4().to_string();
                let download_job = DownloadJob {
                    image_id: dl.image_id,
                    source_image_url: dl.source_image_url,
                    image_path: dl.image_path,
                    parent_job_id: Some(current_id.to_string()),
                    root_job_id: job.root_job_id.clone(),
                    task_id: Some(download_task_id.clone()),
                    max_retries: state.config.task_max_retries,
                    backoff_base: state.config.task_backoff_base,
                };
                let metadata = serde_json::to_value(&download_job)
                    .map_err(|e| format!("Failed to serialize download job: {}", e))?;
                state
                    .queue_backend
                    .push_task(&download_job, "download", metadata, &state.db, Some(&current_id), Some(&current_id), None, Some(dl.image_id), Some(&download_task_id))
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
            }
            total_discovered += 1;
        }
    }

    tracing::info!(total_discovered, "Discover hop {} completed", hop);

    // Submit next hop if within limits
    if hop < max_hops {
        let next_discover_task_id = uuid::Uuid::new_v4().to_string();
        let next_discover_job = DiscoverJob {
            hop: hop + 1,
            max_hops: Some(max_hops),
            seed_limit: Some(seed_limit),
            seed_method: job.seed_method.clone(),
            credential_ids: job.credential_ids.clone(),
            parent_job_id: Some(current_id.clone()),
            task_id: Some(next_discover_task_id.clone()),
            root_job_id: job.root_job_id.clone(),
            max_retries: state.config.task_max_retries,
            backoff_base: state.config.task_backoff_base,
        };
        let metadata = serde_json::to_value(&next_discover_job)
            .map_err(|e| format!("Failed to serialize discover job: {}", e))?;
        state
            .queue_backend
            .push_task(&next_discover_job, "discover", metadata, &state.db, Some(&current_id), Some(&current_id), None, None, Some(&next_discover_task_id))
            .await
            .map_err(|e| format!("Failed to submit next discover task: {}", e))?;

        tracing::info!(next_hop = hop + 1, "Submitted next discover hop");
    } else {
        tracing::info!("Max discover hops ({}) reached, stopping", max_hops);
    }

    Ok(())
}

/// Refresh a Pixiv credential's OAuth token.
pub async fn handle_refresh_pixiv_token(
    job: RefreshPixivTokenJob,
    state: &Arc<WorkerState>,
) -> Result<(), String> {
    let credential_id = job.credential_id;

    let cred = query::pixiv_credential::find_by_id(&state.db, credential_id)
        .await
        .map_err(|e| format!("Failed to fetch credential: {}", e))?
        .ok_or_else(|| format!("Credential {} not found", credential_id))?;

    tracing::info!(
        credential_id,
        pixiv_user_id = %cred.pixiv_user_id,
        "Starting Pixiv token refresh"
    );

    let api = crate::pixiv::create_api(&state.config.pixiv_proxy, &state.config.pixiv_accept_lang).await;

    api.auth(&cred.refresh_token).await.map_err(|e| {
        let msg = format!("Pixiv auth failed for credential {}: {}", credential_id, e);
        tracing::error!("{}", msg);
        msg
    })?;

    let new_refresh_token = api.current_refresh_token().await;
    let new_access_token = api.access_token().await;
    let new_user_id = api.user_id().await;

    let refresh_to_save = new_refresh_token.as_deref().unwrap_or(&cred.refresh_token);
    query::pixiv_credential::update_token(
        &state.db,
        credential_id,
        refresh_to_save,
        new_access_token.as_deref(),
        new_user_id,
    )
    .await
    .map_err(|e| format!("Failed to update token: {}", e))?;

    use crate::db::entities::pixiv_credential;
    query::pixiv_credential::update_status(
        &state.db,
        credential_id,
        pixiv_credential::STATUS_ACTIVE,
    )
    .await
    .map_err(|e| format!("Failed to update status: {}", e))?;

    tracing::info!(
        credential_id,
        pixiv_user_id = %cred.pixiv_user_id,
        "Pixiv token refresh completed"
    );

    Ok(())
}
