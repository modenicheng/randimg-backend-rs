use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use fang::FangError;
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    Unauthorized,
    BadRequest(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "Could not validate credentials".into(),
            ),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Internal(msg) => {
                tracing::error!(
                    error = %msg,
                    status = %StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error"
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".into(),
                )
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<sea_orm::DbErr> for AppError {
    fn from(err: sea_orm::DbErr) -> Self {
        AppError::Internal(err.to_string())
    }
}

impl From<FangError> for AppError {
    fn from(err: FangError) -> Self {
        AppError::Internal(format!("Fang error: {:?}", err))
    }
}
