use axum::{
    Json, Router,
    extract::Path,
    extract::State,
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::WorkerState;
use crate::auth::middleware::AuthUser;
use crate::db::query;
use crate::error::AppError;
use crate::task_queue::jobs::RefreshPixivTokenJob;
use uuid::Uuid;

pub fn routes() -> Router<Arc<WorkerState>> {
    Router::new()
        .route(
            "/pixiv-credential",
            get(list_credentials).post(create_credential),
        )
        .route(
            "/pixiv-credential/{id}",
            get(get_credential)
                .patch(update_credential)
                .delete(delete_credential),
        )
        .route("/pixiv-credential/{id}/refresh", post(refresh_credential))
        .route("/pixiv-credential/{id}/token", get(get_credential_token))
}

#[derive(Deserialize)]
pub struct CreateCredentialRequest {
    pub pixiv_user_id: String,
    pub refresh_token: String,
    pub note: Option<String>,
}

/// POST /pixiv-credential
pub async fn create_credential(
    State(state): State<Arc<WorkerState>>,
    _auth: AuthUser,
    Json(body): Json<CreateCredentialRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.pixiv_user_id.is_empty() {
        return Err(AppError::BadRequest("pixiv_user_id is required".into()));
    }
    if body.refresh_token.is_empty() {
        return Err(AppError::BadRequest("refresh_token is required".into()));
    }

    let cred = query::pixiv_credential::create(
        &state.db,
        &body.pixiv_user_id,
        &body.refresh_token,
        body.note.as_deref(),
    )
    .await
    .map_err(AppError::from)?;

    Ok(Json(credential_to_json(&cred)))
}

/// GET /pixiv-credential
pub async fn list_credentials(
    State(state): State<Arc<WorkerState>>,
    _auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let creds = query::pixiv_credential::find_all(&state.db)
        .await
        .map_err(AppError::from)?;

    let result: Vec<serde_json::Value> = creds.iter().map(credential_to_json).collect();
    Ok(Json(result))
}

/// GET /pixiv-credential/{id}
pub async fn get_credential(
    State(state): State<Arc<WorkerState>>,
    Path(id): Path<i32>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let cred = query::pixiv_credential::find_by_id(&state.db, id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound("Credential not found".into()))?;

    Ok(Json(credential_to_json(&cred)))
}

/// GET /pixiv-credential/{id}/token  — exposes the actual refresh_token (sensitive)
/// This endpoint requires admin-level access control. In the current implementation,
/// all authenticated users can access it. Consider restricting to admin-only.
pub async fn get_credential_token(
    State(state): State<Arc<WorkerState>>,
    Path(id): Path<i32>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    // SECURITY: This endpoint exposes sensitive Pixiv tokens.
    // TODO: Add admin role check when role-based access control is implemented.
    // For now, log access attempts for audit trail.
    tracing::warn!(
        user = %_auth.username,
        credential_id = id,
        "Pixiv token access requested — verify this user should have access"
    );

    let cred = query::pixiv_credential::find_by_id(&state.db, id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound("Credential not found".into()))?;

    Ok(Json(serde_json::json!({
        "id": cred.id,
        "pixiv_user_id": cred.pixiv_user_id,
        "refresh_token": cred.refresh_token,
        "access_token": cred.access_token,
    })))
}

#[derive(Deserialize)]
pub struct UpdateCredentialRequest {
    pub refresh_token: Option<String>,
    pub status: Option<i32>,
    pub note: Option<String>,
}

/// PATCH /pixiv-credential/{id}
pub async fn update_credential(
    State(state): State<Arc<WorkerState>>,
    Path(id): Path<i32>,
    _auth: AuthUser,
    Json(body): Json<UpdateCredentialRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let cred = query::pixiv_credential::update(
        &state.db,
        id,
        body.refresh_token.as_deref(),
        None, // don't touch access_token via this endpoint
        body.status,
        body.note.as_deref().map(Some),
    )
    .await
    .map_err(AppError::from)?
    .ok_or(AppError::NotFound("Credential not found".into()))?;

    Ok(Json(credential_to_json(&cred)))
}

/// DELETE /pixiv-credential/{id}
pub async fn delete_credential(
    State(state): State<Arc<WorkerState>>,
    Path(id): Path<i32>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = query::pixiv_credential::delete(&state.db, id)
        .await
        .map_err(AppError::from)?;

    if !deleted {
        return Err(AppError::NotFound("Credential not found".into()));
    }

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// POST /pixiv-credential/{id}/refresh  — submit a refresh task
pub async fn refresh_credential(
    State(state): State<Arc<WorkerState>>,
    Path(id): Path<i32>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    // Verify credential exists
    let _cred = query::pixiv_credential::find_by_id(&state.db, id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound("Credential not found".into()))?;

    let task_id = Uuid::new_v4().to_string();
    let refresh_job = RefreshPixivTokenJob {
        credential_id: id,
        parent_job_id: None,
        task_id: Some(task_id.clone()),
        max_retries: state.config.task_max_retries,
        backoff_base: state.config.task_backoff_base,
    };
    state
        .queue_backend
        .push_task(
            &refresh_job,
            "refresh_pixiv_token",
            serde_json::to_value(&refresh_job).unwrap_or_default(),
            &state.db,
            None,
            None,
            None,
            None,
            Some(&task_id),
        )
        .await
        .map_err(|e| AppError::Internal(e))?;

    Ok(Json(serde_json::json!({
        "status": "ok",
        "message": "Refresh task submitted",
    })))
}

fn credential_to_json(cred: &crate::db::entities::pixiv_credential::Model) -> serde_json::Value {
    serde_json::json!({
        "id": cred.id,
        "pixiv_user_id": cred.pixiv_user_id,
        "status": cred.status,
        "note": cred.note,
        "last_used_at": cred.last_used_at,
        "last_refreshed_at": cred.last_refreshed_at,
        "created_at": cred.created_at,
        "updated_at": cred.updated_at,
    })
}
