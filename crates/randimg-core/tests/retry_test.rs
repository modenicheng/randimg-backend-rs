use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use randimg_core::task_queue::retry::retry_with_auth_recovery;

#[tokio::test]
async fn test_retry_succeeds_on_first_try() {
    let result = retry_with_auth_recovery(
        "test_op",
        3,
        100,
        || async { Ok::<_, String>("success") },
        || async { Ok::<_, String>(()) },
    )
    .await;

    assert_eq!(result.unwrap(), "success");
}

#[tokio::test]
async fn test_retry_recovers_on_401() {
    let call_count = Arc::new(AtomicU32::new(0));
    let call_count_clone = call_count.clone();
    let recover_count = Arc::new(AtomicU32::new(0));
    let recover_count_clone = recover_count.clone();

    let result = retry_with_auth_recovery(
        "test_op",
        3,
        10, // short backoff for tests
        move || {
            let count = call_count_clone.fetch_add(1, Ordering::SeqCst);
            async move {
                if count == 0 {
                    Err("401 Unauthorized".to_string())
                } else {
                    Ok("success")
                }
            }
        },
        move || {
            recover_count_clone.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, String>(()) }
        },
    )
    .await;

    assert_eq!(result.unwrap(), "success");
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
    assert_eq!(recover_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_retry_exhausted_after_max_attempts() {
    let call_count = Arc::new(AtomicU32::new(0));
    let call_count_clone = call_count.clone();
    let recover_count = Arc::new(AtomicU32::new(0));
    let recover_count_clone = recover_count.clone();

    let result = retry_with_auth_recovery(
        "test_op",
        2,  // max 2 retries
        10, // short backoff
        move || {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
            async { Err::<&str, _>("401 Unauthorized".to_string()) }
        },
        move || {
            recover_count_clone.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, String>(()) }
        },
    )
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("auth error after 2 retries"));
    assert_eq!(call_count.load(Ordering::SeqCst), 3); // initial + 2 retries
    assert_eq!(recover_count.load(Ordering::SeqCst), 2); // recovered twice
}
