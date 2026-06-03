// Pixiv client integration using the pixiv-client crate
pub use pixiv_client::config::{ClientConfig, Config};
pub use pixiv_client::{ApiResponse, PixivApi, PixivError};
pub mod downloader {
    pub use pixiv_client::downloader::*;
}
pub use pixiv_client::models::common::{ImageUrls, MetaPage, MetaSinglePage, Tag};
pub use pixiv_client::models::illust::Illust;
pub use pixiv_client::models::user::UserPreview;

use sea_orm::DatabaseConnection;

use crate::db::query;
use crate::db::entities::pixiv_credential;

/// Create a PixivApi client, optionally configured with a proxy.
///
/// Sets `Accept-Language` header so Pixiv API responses include
/// `translated_name` on tags in the requested language.
pub async fn create_api(proxy: &str, accept_lang: &str) -> PixivApi {
    let api = if proxy.is_empty() {
        PixivApi::new()
    } else {
        let client_config = ClientConfig {
            proxy: Some(proxy.to_string()),
            ..Default::default()
        };
        PixivApi::with_config(Config::default(), client_config)
    };
    if !accept_lang.is_empty() {
        let _ = api.set_accept_lang(accept_lang).await;
    }
    api
}

/// Authenticate a fresh `PixivApi` using a credential from the database.
///
/// - If the credential has a stored `access_token`, uses `set_auth` (no network call).
/// - Otherwise, calls `auth()` which exchanges the refresh token and **persists
///   the resulting tokens back to the database** so they are never lost.
///
/// Returns the credential ID for use with [`recover_auth`] on 401.
pub async fn auth_with_credential(
    api: &PixivApi,
    cred: &pixiv_credential::Model,
    db: &DatabaseConnection,
) -> Result<i32, String> {
    // Only use the fast set_auth path when both the access_token and a valid
    // numeric pixiv_user_id are available.  A non-numeric user_id (e.g. the
    // old placeholder "env_default") would silently become 0, causing Pixiv
    // to return 400 on any endpoint that requires a real user_id.
    let can_fast_path = cred.access_token.is_some()
        && cred.pixiv_user_id.parse::<u64>().is_ok_and(|id| id > 0);

    if can_fast_path {
        let at = cred.access_token.as_deref().unwrap();
        let user_id = cred.pixiv_user_id.parse::<u64>().unwrap();
        api.set_auth(at, &cred.refresh_token, user_id).await;
    } else {
        api.auth(&cred.refresh_token)
            .await
            .map_err(|e| format!("Pixiv auth failed: {}", e))?;
        persist_tokens(api, cred.id, db).await?;
    }
    let _ = query::pixiv_credential::touch_last_used(db, cred.id).await;
    Ok(cred.id)
}

/// Recover from a 401 by re-authenticating and persisting the new tokens.
///
/// Re-fetches the credential from DB to get the latest `refresh_token` (which
/// may have been rotated by a concurrent job), then calls `auth()` and saves
/// the resulting tokens.
pub async fn recover_auth(
    api: &PixivApi,
    credential_id: i32,
    db: &DatabaseConnection,
) -> Result<(), String> {
    let cred = query::pixiv_credential::find_by_id(db, credential_id)
        .await
        .map_err(|e| format!("Failed to fetch credential for auth recovery: {}", e))?
        .ok_or_else(|| format!("Credential {} not found for auth recovery", credential_id))?;

    tracing::info!(
        credential_id,
        "Recovering from 401: re-authenticating with Pixiv"
    );

    api.auth(&cred.refresh_token)
        .await
        .map_err(|e| format!("Pixiv auth recovery failed: {}", e))?;

    persist_tokens(api, credential_id, db).await
}

async fn persist_tokens(
    api: &PixivApi,
    credential_id: i32,
    db: &DatabaseConnection,
) -> Result<(), String> {
    let new_refresh = api.current_refresh_token().await;
    let new_access = api.access_token().await;
    let new_user_id = api.user_id().await;

    let refresh_to_save = new_refresh.as_deref().unwrap_or("");
    query::pixiv_credential::update_token(
        db,
        credential_id,
        refresh_to_save,
        new_access.as_deref(),
        new_user_id,
    )
    .await
    .map_err(|e| format!("Failed to persist refreshed tokens: {}", e))?;

    let _ = query::pixiv_credential::update_status(
        db,
        credential_id,
        pixiv_credential::STATUS_ACTIVE,
    )
    .await;

    Ok(())
}
