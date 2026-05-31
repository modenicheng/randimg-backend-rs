use crate::AppState;
use crate::db::entities::task;
use crate::pixiv::{PixivApi, Illust};
use crate::db::query;
use crate::task_queue;
use sea_orm::*;

pub async fn run(state: &AppState, task: &task::Model) -> Result<(), String> {
    let crawl_type = task.payload["crawl_type"]
        .as_i64()
        .ok_or("missing crawl_type")? as i32;

    // Create and authenticate Pixiv API
    let api = crate::pixiv::create_api(&state.config.pixiv_proxy);
    if !state.config.pixiv_refresh_token.is_empty() {
        api.auth(&state.config.pixiv_refresh_token)
            .await
            .map_err(|e| format!("Pixiv auth failed: {}", e))?;
    }

    match crawl_type {
        0 => crawl_ranking(state, &api, &task.payload).await,
        1 => crawl_user(state, &api, &task.payload).await,
        2 => crawl_bookmarks(state, &api, &task.payload).await,
        _ => Err(format!("Unknown crawl_type: {}", crawl_type)),
    }
}

async fn crawl_ranking(
    state: &AppState,
    api: &PixivApi,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let start_date = payload["target_start_date"].as_str();

    // Extract date portion from datetime string (YYYY-MM-DD HH:MM:SS -> YYYY-MM-DD)
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
            save_illust(state, illust).await?;
        }

        // Check if there's a next page
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
    payload: &serde_json::Value,
) -> Result<(), String> {
    let user_id = payload["target_user_id"]
        .as_str()
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
            save_illust(state, illust).await?;
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
    payload: &serde_json::Value,
) -> Result<(), String> {
    // Use the authenticated user's bookmarks
    let user_id = api
        .user_id()
        .await
        .ok_or("Not authenticated or no user_id")?;

    let tags = payload["target_search_prompt"].as_str();

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
            save_illust(state, illust).await?;
        }

        // Pagination via max_bookmark_id
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

/// Save an Illust to the database and submit download tasks
async fn save_illust(state: &AppState, illust: &Illust) -> Result<(), String> {
    let db = &state.db;

    // Upsert author
    let user = illust.user.as_ref();
    let author_name = user.and_then(|u| u.name.as_deref()).unwrap_or("unknown");
    let author_id_str = user.map(|u| u.id.to_string());

    let author = query::author::find_or_create(db, author_name, Some("pixiv"), author_id_str.as_deref())
        .await
        .map_err(|e| format!("Failed to upsert author: {}", e))?;

    // Determine image URLs for each page
    let pages = get_image_pages(illust);

    for (page_idx, (image_url, _)) in pages.iter().enumerate() {
        let ext = if image_url.contains(".png") { "png" } else { "jpg" };
        let image_path = format!("{}_p{}.{}", illust.id, page_idx, ext);

        let width = illust.width.unwrap_or(0) as i32;
        let height = illust.height.unwrap_or(0) as i32;
        let aspect_ratio = if height > 0 {
            width as f32 / height as f32
        } else {
            0.0
        };

        // Check if image already exists
        let existing = crate::db::entities::image::Entity::find()
            .filter(crate::db::entities::image::Column::ImagePath.eq(&image_path))
            .one(db)
            .await
            .map_err(|e| e.to_string())?;

        if existing.is_some() {
            continue; // Skip existing images
        }

        // Create image record
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
        });

        let image = query::image::create_image(db, &image_data)
            .await
            .map_err(|e| format!("Failed to create image: {}", e))?;

        // Upsert tags and associate
        if let Some(tags) = &illust.tags {
            for tag in tags {
                let tag_model = query::tag::find_or_create(db, &tag.name, tag.translated_name.as_deref())
                    .await
                    .map_err(|e| format!("Failed to upsert tag: {}", e))?;

                // Insert association (ignore duplicate)
                let result = crate::db::entities::image_tag_association::ActiveModel {
                    image_id: Set(image.id),
                    tag_id: Set(tag_model.id),
                }
                .insert(db)
                .await;
                match result {
                    Ok(_) => {}
                    Err(sea_orm::DbErr::RecordNotInserted) => {} // duplicate, ignore
                    Err(e) => return Err(format!("Failed to insert tag association: {}", e)),
                }
            }
        }

        // Submit download task
        task_queue::submit_task(
            db,
            "download",
            serde_json::json!({
                "image_id": image.id,
                "source_image_url": image_url,
                "image_path": image_path,
            }),
            0,
        )
        .await
        .map_err(|e| format!("Failed to submit download task: {}", e))?;
    }

    tracing::info!("Saved illust {} with {} pages", illust.id, pages.len());
    Ok(())
}

/// Get image URLs for all pages of an illustration
fn get_image_pages(illust: &Illust) -> Vec<(String, String)> {
    let mut pages = Vec::new();

    // Multi-page
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

    // Single page
    if pages.is_empty() {
        let url = illust
            .meta_single_page
            .as_ref()
            .and_then(|msp| msp.original_image_url.as_ref())
            .or_else(|| illust.image_urls.as_ref().and_then(|iu| iu.original.as_ref()))
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
