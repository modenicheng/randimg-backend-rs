use axum::{Json, Router, extract::State, routing::post};
use serde::Deserialize;
use std::sync::Arc;

use crate::WorkerState;
use crate::auth::{jwt::create_token, password::verify_password};
use crate::db::query::admin;
use crate::error::AppError;

pub fn routes() -> Router<Arc<WorkerState>> {
    Router::new().route("/token", post(login))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

pub async fn login(
    State(state): State<Arc<WorkerState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let admin = admin::find_by_username(&state.db, &body.username)
        .await
        .map_err(AppError::from)?;

    // Constant-time validation: always run Argon2 to prevent timing attacks
    // that could enumerate valid usernames
    let dummy_hash = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$someinvalidhash";
    let password_ok = match &admin {
        Some(a) => verify_password(&body.password, &a.password),
        None => {
            verify_password(&body.password, dummy_hash);
            false
        }
    };

    if !password_ok {
        return Err(AppError::Unauthorized);
    }

    let admin = admin.unwrap();
    let token = create_token(
        &admin.username,
        &state.config.secret_key,
        state.config.jwt_expire_minutes,
    )?;

    Ok(Json(serde_json::json!({
        "access_token": token,
        "token_type": "bearer"
    })))
}
