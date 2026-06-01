//! Unit tests for AppError: IntoResponse behavior and DbErr conversion.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use randimg_backend_rs::error::AppError;

async fn extract_response(resp: axum::response::Response) -> (StatusCode, String) {
    use http_body_util::BodyExt;
    let status = resp.status();
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    (status, body_str)
}

#[tokio::test]
async fn test_not_found_returns_404() {
    let err = AppError::NotFound("item not found".into());
    let resp = err.into_response();
    let (status, body) = extract_response(resp).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["error"], "item not found");
}

#[tokio::test]
async fn test_unauthorized_returns_401() {
    let err = AppError::Unauthorized;
    let resp = err.into_response();
    let (status, body) = extract_response(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["error"], "Could not validate credentials");
}

#[tokio::test]
async fn test_bad_request_returns_400() {
    let err = AppError::BadRequest("invalid input".into());
    let resp = err.into_response();
    let (status, body) = extract_response(resp).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["error"], "invalid input");
}

#[tokio::test]
async fn test_internal_returns_500() {
    let err = AppError::Internal("something broke".into());
    let resp = err.into_response();
    let (status, body) = extract_response(resp).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    // Internal errors are sanitized — the real message is logged, not returned
    assert_eq!(json["error"], "Internal server error");
}

#[test]
fn test_from_dberr_converts_to_internal() {
    let db_err = sea_orm::DbErr::RecordNotFound("not found".into());
    let app_err: AppError = db_err.into();
    match app_err {
        AppError::Internal(msg) => assert!(msg.contains("not found")),
        _ => panic!("Expected Internal variant"),
    }
}

#[test]
fn test_from_dberr_custom_error() {
    let db_err = sea_orm::DbErr::Custom("custom db error".into());
    let app_err: AppError = db_err.into();
    match app_err {
        AppError::Internal(msg) => assert!(msg.contains("custom db error")),
        _ => panic!("Expected Internal variant"),
    }
}

#[test]
fn test_app_error_is_debug() {
    let err = AppError::BadRequest("test".into());
    let debug_str = format!("{:?}", err);
    assert!(debug_str.contains("BadRequest"));
}
