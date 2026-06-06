use std::sync::Arc;
use std::time::Duration;

use crate::WorkerState;
use crate::db::query;
use crate::db::query::image::SeedMethod;

use super::super::jobs::*;
use super::super::retry::retry_with_auth_recovery;
use super::helpers::save_illust;

/// Discover related illustrations via Pixiv related-illust API.
pub async fn handle_discover(job: DiscoverJob, state: &Arc<WorkerState>) -> Result<(), String> {
    let current_id = job
        .task_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

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
    let api = crate::pixiv::create_api(
        &state.config.pixiv_proxy,
        &state.config.pixiv_accept_lang,
        Some(Duration::from_secs(state.config.pixiv_timeout_secs)),
    )
    .await;
    let credential_id = if let Some(ref ids) = job.credential_ids {
        if !ids.is_empty() {
            let creds = query::pixiv_credential::find_active_by_ids(&state.db, ids)
                .await
                .map_err(|e| format!("Failed to fetch credentials by IDs: {}", e))?;
            if creds.is_empty() {
                return Err(format!(
                    "No active credentials found among specified IDs: {:?}",
                    ids
                ));
            }
            let cred = creds.into_iter().next().unwrap();
            crate::pixiv::auth_with_credential(&api, &cred, &state.db)
                .await
                .map_err(|e| e.to_string())?
        } else {
            let cred = query::pixiv_credential::find_one_active_random(&state.db)
                .await
                .map_err(|e| format!("Failed to fetch credential: {}", e))?
                .ok_or("No active Pixiv credentials found")?;
            crate::pixiv::auth_with_credential(&api, &cred, &state.db)
                .await
                .map_err(|e| e.to_string())?
        }
    } else {
        let cred = query::pixiv_credential::find_one_active_random(&state.db)
            .await
            .map_err(|e| format!("Failed to fetch credential: {}", e))?
            .ok_or("No active Pixiv credentials found")?;
        crate::pixiv::auth_with_credential(&api, &cred, &state.db)
            .await
            .map_err(|e| e.to_string())?
    };

    let mut total_discovered = 0u32;
    for seed in &seeds {
        let source_id = seed
            .source_id
            .ok_or_else(|| format!("Seed {} missing source_id", seed.id))?;

        let resp = retry_with_auth_recovery(
            "illust_related",
            state.config.auth_max_retries,
            state.config.auth_backoff_base_ms,
            || async {
                api.illust_related(source_id as u64)
                    .await
                    .map_err(|e| e.to_string())
            },
            || async {
                crate::pixiv::recover_auth(&api, credential_id, &state.db)
                    .await
                    .map_err(|e| e.to_string())
            },
        )
        .await?;

        let data = resp.data.ok_or("No data in illust_related response")?;

        for illust in &data.illusts {
            let illust_id = illust.id;
            match state.discover_cache.entry(illust_id.to_string()) {
                dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                    if entry.get().elapsed() < Duration::from_secs(3600) {
                        tracing::debug!(illust_id, "Skipping duplicate discover");
                        continue;
                    }
                    entry.insert(std::time::Instant::now());
                }
                dashmap::mapref::entry::Entry::Vacant(entry) => {
                    entry.insert(std::time::Instant::now());
                }
            }

            let downloads = save_illust(
                state,
                illust,
                job.illust_type_filter.clone(),
                job.exclude_r18,
                job.exclude_ai,
            )
            .await?;
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
                    .push_task(
                        &download_job,
                        "download",
                        metadata,
                        &state.db,
                        Some(&current_id),
                        Some(&current_id),
                        None,
                        Some(dl.image_id),
                        Some(&download_task_id),
                    )
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
            illust_type_filter: job.illust_type_filter.clone(),
            exclude_r18: job.exclude_r18,
            exclude_ai: job.exclude_ai,
            max_retries: state.config.task_max_retries,
            backoff_base: state.config.task_backoff_base,
        };
        let metadata = serde_json::to_value(&next_discover_job)
            .map_err(|e| format!("Failed to serialize discover job: {}", e))?;
        state
            .queue_backend
            .push_task(
                &next_discover_job,
                "discover",
                metadata,
                &state.db,
                Some(&current_id),
                Some(&current_id),
                None,
                None,
                Some(&next_discover_task_id),
            )
            .await
            .map_err(|e| format!("Failed to submit next discover task: {}", e))?;

        tracing::info!(next_hop = hop + 1, "Submitted next discover hop");
    } else {
        tracing::info!("Max discover hops ({}) reached, stopping", max_hops);
    }

    Ok(())
}
