use std::sync::Arc;
use std::time::Duration;

use crate::WorkerState;
use crate::db::query;

use super::super::jobs::*;

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

    let api = crate::pixiv::create_api(
        &state.config.pixiv_proxy,
        &state.config.pixiv_accept_lang,
        Some(Duration::from_secs(state.config.pixiv_timeout_secs)),
    )
    .await;

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
