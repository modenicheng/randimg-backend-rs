use std::sync::Arc;
use std::time::Duration;

use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

use crate::WorkerState;
use crate::db::query;
use crate::pixiv::PixivApi;

use super::super::CrawlType;
use super::super::jobs::*;
use super::super::retry::retry_with_auth_recovery;
use super::helpers::{save_illust, extract_param_from_url};

// ---------------------------------------------------------------------------
// Crawl handlers
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
    let api = crate::pixiv::create_api(
        &state.config.pixiv_proxy,
        &state.config.pixiv_accept_lang,
        Some(Duration::from_secs(state.config.pixiv_timeout_secs)),
    )
    .await;
    let credential_id = if let Some(cred) = query::pixiv_credential::find_one_active_random(&state.db)
        .await
        .map_err(|e| format!("Failed to fetch credential: {}", e))?
    {
        crate::pixiv::auth_with_credential(&api, &cred, &state.db).await.map_err(|e| e.to_string())?
    } else {
        return Err("No active Pixiv credentials found".into());
    };

    let result = match CrawlType::try_from(crawl_type)? {
        CrawlType::Ranking => crawl_ranking(&**state, &api, credential_id, &job, &current_id).await,
        CrawlType::User => crawl_user(&**state, &api, credential_id, &job, &current_id).await,
        CrawlType::Bookmarks => crawl_bookmarks(&**state, &api, credential_id, &job, &current_id).await,
    };

    // Update crawler status based on result
    match &result {
        Ok(()) => {
            let total = crate::db::entities::image::Entity::find()
                .filter(crate::db::entities::image::Column::SourceId.is_not_null())
                .count(&state.db)
                .await
                .unwrap_or(0) as i32;

            if let Err(e) = query::crawler::mark_completed(&state.db, crawler_id, total).await {
                tracing::warn!(crawler_id, error = %e, "Failed to mark crawler as completed");
            }

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
                illust_type_filter: job.illust_type_filter.clone(),
                exclude_r18: job.exclude_r18,
                exclude_ai: job.exclude_ai,
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
            if let Err(e) = query::crawler::mark_failed(&state.db, crawler_id).await {
                tracing::warn!(crawler_id, error = %e, "Failed to mark crawler as failed");
            }
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
        let resp = retry_with_auth_recovery(
            "illust_ranking",
            state.config.auth_max_retries,
            state.config.auth_backoff_base_ms,
            || async { api.illust_ranking(Some(mode), date, Some(offset)).await.map_err(|e| e.to_string()) },
            || async { crate::pixiv::recover_auth(api, credential_id, &state.db).await.map_err(|e| e.to_string()) },
        ).await?;

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

    let illust_type: Option<&str> = job.illust_type_filter.as_ref()
        .and_then(|f| {
            if f.len() == 1 && matches!(f[0].as_str(), "illust" | "manga") {
                Some(f[0].as_str())
            } else {
                None
            }
        });
    let max_pages = job.max_pages.unwrap_or(0);

    let mut offset = 0u32;
    let mut pages_processed = 0u32;
    loop {
        let resp = retry_with_auth_recovery(
            "user_illusts",
            state.config.auth_max_retries,
            state.config.auth_backoff_base_ms,
            || async { api.user_illusts(user_id, illust_type, Some(offset)).await.map_err(|e| e.to_string()) },
            || async { crate::pixiv::recover_auth(api, credential_id, &state.db).await.map_err(|e| e.to_string()) },
        ).await?;

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
        let resp = retry_with_auth_recovery(
            "user_bookmarks_illust",
            state.config.auth_max_retries,
            state.config.auth_backoff_base_ms,
            || async { api.user_bookmarks_illust(user_id, Some("public"), max_bookmark_id, tags).await.map_err(|e| e.to_string()) },
            || async { crate::pixiv::recover_auth(api, credential_id, &state.db).await.map_err(|e| e.to_string()) },
        ).await?;

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
