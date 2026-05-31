use axum::{extract::State, routing::post, Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::{jwt::create_token, password::verify_password};
use crate::db::query::admin;
use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/token", post(login))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let admin = admin::find_by_username(&state.db, &body.username)
        .await
        .map_err(AppError::from)?;

    let admin = admin.ok_or(AppError::Unauthorized)?;

    if !verify_password(&body.password, &admin.password) {
        return Err(AppError::Unauthorized);
    }

    let token = create_token(
        &admin.username,
        &state.config.secret_key,
        state.config.jwt_expire_minutes,
    );

    Ok(Json(serde_json::json!({
        "access_token": token,
        "token_type": "bearer"
    })))
}
