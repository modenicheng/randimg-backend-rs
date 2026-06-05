use std::time::Duration;

/// Retry an async operation with automatic auth recovery on 401 errors.
///
/// If the operation fails with a 401 or "unauthorized" error, calls `recover`
/// to refresh the authentication token, then retries with exponential backoff.
/// Non-auth errors are returned immediately without retry.
pub async fn retry_with_auth_recovery<F, Fut, T, R, RecFut>(
    operation_name: &str,
    max_retries: u32,
    backoff_base_ms: u64,
    f: F,
    recover: R,
) -> Result<T, String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
    R: Fn() -> RecFut,
    RecFut: std::future::Future<Output = Result<(), String>>,
{
    let mut retries = 0u32;
    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                let err_str = e.to_lowercase();
                if err_str.contains("401") || err_str.contains("unauthorized") {
                    if retries >= max_retries {
                        tracing::warn!(
                            operation = operation_name,
                            retries,
                            max_retries,
                            "Auth recovery exhausted after {max_retries} retries"
                        );
                        return Err(format!(
                            "{operation_name}: auth error after {max_retries} retries: {e}"
                        ));
                    }
                    tracing::warn!(
                        operation = operation_name,
                        attempt = retries + 1,
                        max_retries,
                        "Auth error in {operation_name}, refreshing token (attempt {}/{max_retries})",
                        retries + 1,
                    );
                    recover().await?;
                    retries += 1;
                    let delay = Duration::from_millis(backoff_base_ms * 2u64.pow(retries - 1));
                    tokio::time::sleep(delay).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
}
