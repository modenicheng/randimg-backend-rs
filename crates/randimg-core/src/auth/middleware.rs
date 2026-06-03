use axum::{
    RequestPartsExt,
    extract::FromRequestParts,
    extract::{FromRef, State},
    http::{StatusCode, request::Parts},
};
use axum_extra::TypedHeader;
use axum_extra::headers::{Authorization, authorization::Bearer};

use super::jwt::verify_token;
use crate::WorkerState;
use std::sync::Arc;

/// Required auth extractor
pub struct AuthUser {
    pub username: String,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Arc<WorkerState>: FromRef<S>,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        let State(state): State<Arc<WorkerState>> = parts
            .extract_with_state(_state)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let claims = verify_token(bearer.token(), &state.config.secret_key)
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        Ok(AuthUser {
            username: claims.sub,
        })
    }
}

/// Optional auth extractor
pub struct OptionalAuthUser {
    pub username: Option<String>,
}

impl<S> FromRequestParts<S> for OptionalAuthUser
where
    S: Send + Sync,
    Arc<WorkerState>: FromRef<S>,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header: Option<TypedHeader<Authorization<Bearer>>> = parts.extract().await.ok();

        if let Some(TypedHeader(Authorization(bearer))) = auth_header {
            let State(state): State<Arc<WorkerState>> = parts
                .extract_with_state(_state)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            if let Ok(claims) = verify_token(bearer.token(), &state.config.secret_key) {
                return Ok(OptionalAuthUser {
                    username: Some(claims.sub),
                });
            }
        }

        Ok(OptionalAuthUser { username: None })
    }
}
