use std::sync::Arc;

use apalis::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};

use crate::AppState;
use crate::db::query;
use crate::db::query::image::SeedMethod;
use crate::db_backend::JobStorage;
use crate::pixiv::PixivApi;

use super::jobs::*;

// ---------------------------------------------------------------------------
// Job handlers
// ---------------------------------------------------------------------------

/// Crawl Pixiv illustrations (by user, ranking, or bookmarks).
pub async fn handle_crawl(
    job: CrawlJob,
    state: Data<Arc<AppState>>,
    storage: Data<JobStorage>,
) -> Result<(), BoxDynError> {
    let crawl_type = job.crawl_type;
    let crawler_id = job.crawler_id;

    // Mark crawler as running
    query::crawler::mark_running(&state.db, crawler_id)
        .await
        .map_err(|e| format!("Failed to mark crawler running: {}", e))?;

    // Create and authenticate Pixiv API
    let api = crate::pixiv::create_api(&state.config.pixiv_proxy);
    if let Some(cred) = query::pixiv_credential::find_one_active_random(&state.db)
        .await
        .map_err(|e| format!("Failed to fetch credential: {}", e))?
    {
        api.auth(&cred.refresh_token)
            .await
            .map_err(|e| format!("Pixiv auth failed: {}", e))?;
        if let Some(new_token) = api.current_refresh_token().await {
            let _ =
                query::pixiv_credential::update_token(&state.db, cred.id, &new_token, None).await;
        }
        let _ = query::pixiv_credential::touch_last_used(&state.db, cred.id).await;
    }

    let result = match crawl_type {
        0 => crawl_ranking(&state, &api, &job, &storage).await,
        1 => crawl_user(&state, &api, &job, &storage).await,
        2 => crawl_bookmarks(&state, &api, &job, &storage).await,
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

            // Trigger autonomous discover for next-hop crawling
            if let Err(e) = storage
                .push_discover(DiscoverJob {
                    hop: 0,
                    max_hops: None,
                    seed_limit: None,
                    seed_method: None,
                })
                .await
            {
                tracing::error!("Failed to submit discover task after crawl: {}", e);
            }
        }
        Err(_) => {
            let _ = query::crawler::mark_failed(&state.db, crawler_id).await;
        }
    }

    result.map_err(Into::into)
}

async fn crawl_ranking(
    state: &AppState,
    api: &PixivApi,
    job: &CrawlJob,
    storage: &JobStorage,
) -> Result<(), String> {
    let start_date = job.target_start_date.as_deref();
    let date = start_date.map(|s| if s.len() >= 10 { &s[..10] } else { s });
    let mode = "day";

    let mut offset = 0u32;
    loop {
        let resp = api
            .illust_ranking(Some(mode), date, Some(offset))
            .await
            .map_err(|e| format!("Ranking API error: {}", e))?;

        let data = resp.data.ok_or("No data in ranking response")?;
        let illusts = data.illusts;

        if illusts.is_empty() {
            break;
        }

        for illust in &illusts {
            let downloads = save_illust(state, illust).await?;
            for dl in downloads {
                storage
                    .push_download(DownloadJob {
                        image_id: dl.image_id,
                        source_image_url: dl.source_image_url,
                        image_path: dl.image_path,
                    })
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
            }
        }

        if data.next_url.is_none() {
            break;
        }
        offset += illusts.len() as u32;
    }

    Ok(())
}

async fn crawl_user(
    state: &AppState,
    api: &PixivApi,
    job: &CrawlJob,
    storage: &JobStorage,
) -> Result<(), String> {
    let user_id = job
        .target_user_id
        .as_deref()
        .ok_or("missing target_user_id")?
        .parse::<u64>()
        .map_err(|_| "invalid target_user_id")?;

    let mut offset = 0u32;
    loop {
        let resp = api
            .user_illusts(user_id, Some("illust"), Some(offset))
            .await
            .map_err(|e| format!("User illusts API error: {}", e))?;

        let data = resp.data.ok_or("No data in user_illusts response")?;
        let illusts = data.illusts;

        if illusts.is_empty() {
            break;
        }

        for illust in &illusts {
            let downloads = save_illust(state, illust).await?;
            for dl in downloads {
                storage
                    .push_download(DownloadJob {
                        image_id: dl.image_id,
                        source_image_url: dl.source_image_url,
                        image_path: dl.image_path,
                    })
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
            }
        }

        if data.next_url.is_none() {
            break;
        }
        offset += illusts.len() as u32;
    }

    Ok(())
}

async fn crawl_bookmarks(
    state: &AppState,
    api: &PixivApi,
    job: &CrawlJob,
    storage: &JobStorage,
) -> Result<(), String> {
    let user_id = api
        .user_id()
        .await
        .ok_or("Not authenticated or no user_id")?;

    let tags = job.target_search_prompt.as_deref();

    let mut max_bookmark_id: Option<u64> = None;
    loop {
        let resp = api
            .user_bookmarks_illust(user_id, Some("public"), max_bookmark_id, tags)
            .await
            .map_err(|e| format!("Bookmarks API error: {}", e))?;

        let data = resp.data.ok_or("No data in bookmarks response")?;
        let illusts = data.illusts;

        if illusts.is_empty() {
            break;
        }

        for illust in &illusts {
            let downloads = save_illust(state, illust).await?;
            for dl in downloads {
                storage
                    .push_download(DownloadJob {
                        image_id: dl.image_id,
                        source_image_url: dl.source_image_url,
                        image_path: dl.image_path,
                    })
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
            }
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
    state: &AppState,
    illust: &crate::pixiv::Illust,
) -> Result<Vec<DownloadInfo>, String> {
    let db = &state.db;
    let mut downloads = Vec::new();

    let user = illust.user.as_ref();
    let author_name = user.and_then(|u| u.name.as_deref()).unwrap_or("unknown");
    let author_id_str = user.map(|u| u.id.to_string());

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

        let width = illust.width.unwrap_or(0) as i32;
        let height = illust.height.unwrap_or(0) as i32;
        let aspect_ratio = if height > 0 {
            width as f32 / height as f32
        } else {
            0.0
        };

        let existing = crate::db::entities::image::Entity::find()
            .filter(crate::db::entities::image::Column::ImagePath.eq(&image_path))
            .one(db)
            .await
            .map_err(|e| e.to_string())?;

        if existing.is_some() {
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

/// Download a single image from Pixiv to local disk.
pub async fn handle_download(
    job: DownloadJob,
    state: Data<Arc<AppState>>,
) -> Result<(), BoxDynError> {
    let client = reqwest::Client::new();
    let resp = client
        .get(&job.source_image_url)
        .header("Referer", "https://app-api.pixiv.net/")
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Download failed with status: {}", resp.status()).into());
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    let file_path = format!("{}/{}", state.config.image_dir, job.image_path);

    if let Some(parent) = std::path::Path::new(&file_path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    tokio::fs::write(&file_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write file: {}", e))?;

    tracing::info!("Downloaded image {} to {}", job.image_id, file_path);
    Ok(())
}

/// Extract color palette from a downloaded image.
pub async fn handle_color_extract(
    job: ColorExtractJob,
    state: Data<Arc<AppState>>,
) -> Result<(), BoxDynError> {
    let file_path = format!("{}/{}", state.config.image_dir, job.image_path);
    let img = ::image::open(&file_path).map_err(|e| format!("Failed to open image: {}", e))?;

    let colors = crate::color::extract_theme_colors(&img);

    // Update images table
    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img_model) = Image::find_by_id(job.image_id)
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
        .filter(image_color_palette::Column::ImageId.eq(job.image_id))
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
            image_id: Set(job.image_id),
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
pub async fn handle_upload(job: UploadJob, state: Data<Arc<AppState>>) -> Result<(), BoxDynError> {
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
    state: Data<Arc<AppState>>,
) -> Result<(), BoxDynError> {
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
pub async fn handle_discover(
    job: DiscoverJob,
    state: Data<Arc<AppState>>,
    storage: Data<JobStorage>,
) -> Result<(), BoxDynError> {
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

    // Create and authenticate Pixiv API
    let api = crate::pixiv::create_api(&state.config.pixiv_proxy);
    if let Some(cred) = query::pixiv_credential::find_one_active_random(&state.db)
        .await
        .map_err(|e| format!("Failed to fetch credential: {}", e))?
    {
        api.auth(&cred.refresh_token)
            .await
            .map_err(|e| format!("Pixiv auth failed: {}", e))?;
        if let Some(new_token) = api.current_refresh_token().await {
            let _ =
                query::pixiv_credential::update_token(&state.db, cred.id, &new_token, None).await;
        }
        let _ = query::pixiv_credential::touch_last_used(&state.db, cred.id).await;
    }

    let mut total_discovered = 0u32;
    for seed in &seeds {
        let source_id = seed
            .source_id
            .ok_or_else(|| format!("Seed {} missing source_id", seed.id))?;

        let resp = api
            .illust_related(source_id as u64)
            .await
            .map_err(|e| format!("illust_related API error: {}", e))?;

        let data = resp.data.ok_or("No data in illust_related response")?;

        for illust in &data.illusts {
            let downloads = save_illust(&state, illust).await?;
            for dl in downloads {
                storage
                    .push_download(DownloadJob {
                        image_id: dl.image_id,
                        source_image_url: dl.source_image_url,
                        image_path: dl.image_path,
                    })
                    .await
                    .map_err(|e| format!("Failed to submit download task: {}", e))?;
            }
            total_discovered += 1;
        }
    }

    tracing::info!(total_discovered, "Discover hop {} completed", hop);

    // Submit next hop if within limits
    if hop < max_hops {
        storage
            .push_discover(DiscoverJob {
                hop: hop + 1,
                max_hops: Some(max_hops),
                seed_limit: Some(seed_limit),
                seed_method: job.seed_method.clone(),
            })
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
    state: Data<Arc<AppState>>,
) -> Result<(), BoxDynError> {
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

    let api = crate::pixiv::create_api(&state.config.pixiv_proxy);

    api.auth(&cred.refresh_token).await.map_err(|e| {
        let msg = format!("Pixiv auth failed for credential {}: {}", credential_id, e);
        tracing::error!("{}", msg);
        msg
    })?;

    let new_refresh_token = api.current_refresh_token().await;
    let new_access_token = api.access_token().await;

    let refresh_to_save = new_refresh_token.as_deref().unwrap_or(&cred.refresh_token);
    query::pixiv_credential::update_token(
        &state.db,
        credential_id,
        refresh_to_save,
        new_access_token.as_deref(),
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
