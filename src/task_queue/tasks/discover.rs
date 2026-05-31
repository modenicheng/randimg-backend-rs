use crate::AppState;
use crate::db::entities::task;
use crate::db::query;
use crate::task_queue;

pub async fn run(state: &AppState, task: &task::Model) -> Result<(), String> {
    let hop = task.payload["hop"].as_i64().unwrap_or(0) as u32;
    let max_hops = state.config.max_discover_hops;
    let seed_limit = state.config.discover_seed_limit;

    tracing::info!(hop, max_hops, seed_limit, "Starting discover task");

    // 1. Select seed images by popularity
    let seeds = query::image::find_discover_seeds(&state.db, seed_limit)
        .await
        .map_err(|e| format!("Failed to find discover seeds: {}", e))?;

    if seeds.is_empty() {
        tracing::info!("No discover seeds found, skipping");
        return Ok(());
    }

    tracing::info!(count = seeds.len(), "Selected discover seeds");

    // 2. Create and authenticate Pixiv API
    let api = crate::pixiv::create_api(&state.config.pixiv_proxy);
    if !state.config.pixiv_refresh_token.is_empty() {
        api.auth(&state.config.pixiv_refresh_token)
            .await
            .map_err(|e| format!("Pixiv auth failed: {}", e))?;
    }

    // 3. For each seed, fetch related illusts and save them
    let mut total_discovered = 0u32;
    for seed in &seeds {
        let source_id = seed
            .source_id
            .ok_or_else(|| format!("Seed {} missing source_id", seed.id))?;

        let related = fetch_related(&api, source_id as u64).await?;

        for illust in &related {
            super::crawl::save_illust(state, illust).await?;
            total_discovered += 1;
        }
    }

    tracing::info!(total_discovered, "Discover hop {} completed", hop);

    // 4. Submit next hop if within limits
    if hop < max_hops {
        task_queue::submit_task(
            &state.db,
            "discover",
            serde_json::json!({ "hop": hop + 1 }),
            0,
        )
        .await
        .map_err(|e| format!("Failed to submit next discover task: {}", e))?;

        tracing::info!(next_hop = hop + 1, "Submitted next discover hop");
    } else {
        tracing::info!("Max discover hops ({}) reached, stopping", max_hops);
    }

    Ok(())
}

/// Fetch related illusts from Pixiv API.
/// Uses the /v2/illust/related endpoint, paginates through all results.
async fn fetch_related(
    api: &crate::pixiv::PixivApi,
    illust_id: u64,
) -> Result<Vec<crate::pixiv::Illust>, String> {
    let resp = api
        .illust_related(illust_id)
        .await
        .map_err(|e| format!("illust_related API error: {}", e))?;

    let data = resp.data.ok_or("No data in illust_related response")?;
    Ok(data.illusts)
}
