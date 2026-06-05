use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub iss: String,
    pub aud: String,
}

pub fn create_token(username: &str, secret: &str, expire_minutes: u64) -> Result<String, AppError> {
    let expiration = Utc::now()
        .checked_add_signed(Duration::minutes(expire_minutes as i64))
        .ok_or_else(|| AppError::Internal("Invalid timestamp".into()))?
        .timestamp() as usize;

    let claims = Claims {
        sub: username.to_string(),
        exp: expiration,
        iss: "randimg".to_string(),
        aud: "randimg-api".to_string(),
    };

    let mut header = Header::default();
    header.kid = Some("randimg-main".to_string());
    encode(
        &header,
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("JWT encoding failed: {}", e)))
}

pub fn verify_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let mut validation = Validation::default();
    validation.set_issuer(&["randimg"]);
    validation.set_audience(&["randimg-api"]);
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;
    Ok(token_data.claims)
}
