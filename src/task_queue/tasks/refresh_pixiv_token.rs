use crate::db::entities::pixiv_credential;
use crate::db::entities::task;
use crate::db::query;
use crate::AppState;

pub async fn run(state: &AppState, task: &task::Model) -> Result<(), String> {
    let credential_id = task.payload["credential_id"]
        .as_i64()
        .ok_or("missing credential_id")? as i32;

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

    // Authenticate with the stored refresh token
    api.auth(&cred.refresh_token)
        .await
        .map_err(|e| {
            let msg = format!("Pixiv auth failed for credential {}: {}", credential_id, e);
            tracing::error!("{}", msg);
            msg
        })?;

    // Capture possibly-rotated refresh token
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

    // Mark as active on successful refresh
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
