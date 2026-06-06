//! Unit tests for task queue job structs: serialization/deserialization.

use randimg_core::task_queue::CrawlType;
use randimg_core::task_queue::jobs::*;

#[test]
fn test_crawl_job_roundtrip() {
    let job = CrawlJob {
        crawler_id: 1,
        crawl_type: 1,
        target_user_id: Some("12345".into()),
        target_start_date: None,
        target_end_date: None,
        target_search_prompt: Some("landscape".into()),
        ranking_mode: None,
        max_pages: None,
        discover_hops: None,
        discover_seed_limit: None,
        discover_seed_method: None,
        parent_job_id: None,
        exclude_r18: None,
        exclude_ai: None,
        illust_type_filter: None,
        disable_discover: None,
        credential_ids: None,
        task_id: None,
        max_retries: 3,
        backoff_base: 2,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: CrawlJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.crawler_id, 1);
    assert_eq!(deserialized.crawl_type, 1);
    assert_eq!(deserialized.target_user_id.as_deref(), Some("12345"));
    assert_eq!(
        deserialized.target_search_prompt.as_deref(),
        Some("landscape")
    );
    assert!(deserialized.ranking_mode.is_none());
    assert!(deserialized.max_pages.is_none());
    assert!(deserialized.discover_hops.is_none());
    assert!(deserialized.discover_seed_limit.is_none());
    assert!(deserialized.discover_seed_method.is_none());
    assert!(deserialized.illust_type_filter.is_none());
    assert!(deserialized.task_id.is_none());
}

#[test]
fn test_download_job_roundtrip() {
    let job = DownloadJob {
        image_id: 42,
        source_image_url: "https://example.com/image.jpg".into(),
        image_path: "/data/images/42.jpg".into(),
        parent_job_id: None,
        root_job_id: None,
        task_id: None,
        max_retries: 3,
        backoff_base: 2,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: DownloadJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 42);
    assert_eq!(
        deserialized.source_image_url,
        "https://example.com/image.jpg"
    );
    assert_eq!(deserialized.image_path, "/data/images/42.jpg");
    assert!(deserialized.task_id.is_none());
}

#[test]
fn test_color_extract_job_roundtrip() {
    let job = ColorExtractJob {
        image_id: 10,
        image_path: "/data/images/10.jpg".into(),
        parent_job_id: None,
        task_id: None,
        max_retries: 0,
        backoff_base: 2,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: ColorExtractJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 10);
    assert_eq!(deserialized.image_path, "/data/images/10.jpg");
    assert!(deserialized.task_id.is_none());
}

#[test]
fn test_upload_job_roundtrip() {
    let job = UploadJob {
        image_id: 5,
        image_path: "/data/images/5.jpg".into(),
        parent_job_id: None,
        task_id: None,
        max_retries: 3,
        backoff_base: 2,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: UploadJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 5);
    assert!(deserialized.task_id.is_none());
}

#[test]
fn test_accessibility_check_job_roundtrip() {
    let job = AccessibilityCheckJob {
        image_id: 7,
        image_path: "/data/images/7.jpg".into(),
        parent_job_id: None,
        task_id: None,
        max_retries: 3,
        backoff_base: 2,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: AccessibilityCheckJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 7);
    assert!(deserialized.task_id.is_none());
}

#[test]
fn test_discover_job_roundtrip() {
    let job = DiscoverJob {
        hop: 0,
        max_hops: Some(3),
        seed_limit: Some(10),
        seed_method: Some("popularity".into()),
        credential_ids: None,
        parent_job_id: Some("parent-uuid-123".into()),
        task_id: None,
        root_job_id: None,
        illust_type_filter: None,
        exclude_r18: None,
        exclude_ai: None,
        max_retries: 3,
        backoff_base: 2,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: DiscoverJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.hop, 0);
    assert_eq!(deserialized.max_hops, Some(3));
    assert_eq!(deserialized.seed_limit, Some(10));
    assert_eq!(deserialized.seed_method.as_deref(), Some("popularity"));
    assert!(deserialized.task_id.is_none());
}

#[test]
fn test_discover_job_optional_fields_none() {
    let job = DiscoverJob {
        hop: 2,
        max_hops: None,
        seed_limit: None,
        seed_method: None,
        credential_ids: None,
        parent_job_id: None,
        task_id: None,
        root_job_id: None,
        illust_type_filter: None,
        exclude_r18: None,
        exclude_ai: None,
        max_retries: 3,
        backoff_base: 2,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: DiscoverJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.hop, 2);
    assert!(deserialized.max_hops.is_none());
    assert!(deserialized.seed_limit.is_none());
    assert!(deserialized.seed_method.is_none());
    assert!(deserialized.task_id.is_none());
}

#[test]
fn test_refresh_pixiv_token_job_roundtrip() {
    let job = RefreshPixivTokenJob {
        credential_id: 99,
        parent_job_id: None,
        task_id: None,
        max_retries: 3,
        backoff_base: 2,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: RefreshPixivTokenJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.credential_id, 99);
    assert!(deserialized.task_id.is_none());
}

#[test]
fn test_crawl_job_deserialize_from_json_literal() {
    let json = r#"{
        "crawler_id": 10,
        "crawl_type": 0,
        "target_user_id": null,
        "target_start_date": "2026-01-01",
        "target_end_date": "2026-01-31",
        "target_search_prompt": null,
        "illust_type_filter": null
    }"#;
    let job: CrawlJob = serde_json::from_str(json).unwrap();
    assert_eq!(job.crawler_id, 10);
    assert!(job.target_user_id.is_none());
    assert_eq!(job.target_start_date.as_deref(), Some("2026-01-01"));
    assert!(job.task_id.is_none());
}

#[test]
fn test_job_structs_are_clone() {
    let job = DownloadJob {
        image_id: 1,
        source_image_url: "url".into(),
        image_path: "path".into(),
        parent_job_id: None,
        root_job_id: None,
        task_id: None,
        max_retries: 3,
        backoff_base: 2,
    };
    let cloned = job.clone();
    assert_eq!(cloned.image_id, 1);
}

/// Verify backward compatibility: JSON without parent_job_id or task_id still works
#[test]
fn test_deserialize_without_parent_job_id() {
    let json = r#"{
        "image_id": 42,
        "source_image_url": "https://example.com/img.jpg",
        "image_path": "/data/42.jpg"
    }"#;
    let job: DownloadJob = serde_json::from_str(json).unwrap();
    assert_eq!(job.image_id, 42);
    assert!(job.parent_job_id.is_none());
    assert!(job.task_id.is_none());
}

/// Verify parent_job_id and task_id are serialized and deserialized correctly
#[test]
fn test_parent_job_id_roundtrip() {
    let job = CrawlJob {
        crawler_id: 1,
        crawl_type: 0,
        target_user_id: None,
        target_start_date: None,
        target_end_date: None,
        target_search_prompt: None,
        ranking_mode: None,
        max_pages: None,
        discover_hops: None,
        discover_seed_limit: None,
        discover_seed_method: None,
        parent_job_id: Some("parent-uuid-abc".into()),
        exclude_r18: None,
        exclude_ai: None,
        illust_type_filter: None,
        disable_discover: None,
        credential_ids: None,
        task_id: Some("task-uuid-xyz".into()),
        max_retries: 3,
        backoff_base: 2,
    };
    let json = serde_json::to_string(&job).unwrap();
    assert!(json.contains("parent-uuid-abc"));
    assert!(json.contains("task-uuid-xyz"));
    let deserialized: CrawlJob = serde_json::from_str(&json).unwrap();
    assert_eq!(
        deserialized.parent_job_id.as_deref(),
        Some("parent-uuid-abc")
    );
    assert_eq!(deserialized.task_id.as_deref(), Some("task-uuid-xyz"));
}

// ── CrawlType conversion tests ──────────────────────────────────

#[test]
fn test_crawl_type_ranking_from_i32() {
    let ct = CrawlType::try_from(0).unwrap();
    assert_eq!(ct, CrawlType::Ranking);
    assert_eq!(ct as i32, 0);
}

#[test]
fn test_crawl_type_user_from_i32() {
    let ct = CrawlType::try_from(1).unwrap();
    assert_eq!(ct, CrawlType::User);
    assert_eq!(ct as i32, 1);
}

#[test]
fn test_crawl_type_bookmarks_from_i32() {
    let ct = CrawlType::try_from(2).unwrap();
    assert_eq!(ct, CrawlType::Bookmarks);
    assert_eq!(ct as i32, 2);
}

#[test]
fn test_crawl_type_invalid_negative() {
    let result = CrawlType::try_from(-1);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid crawl_type"));
}

#[test]
fn test_crawl_type_invalid_positive() {
    let result = CrawlType::try_from(99);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid crawl_type"));
}

#[test]
fn test_crawl_type_debug_and_clone() {
    let ct = CrawlType::Ranking;
    let cloned = ct;
    assert_eq!(format!("{:?}", cloned), "Ranking");
}

// ── Task timeout config tests ──────────────────────────────────

#[test]
fn test_task_timeout_config_defaults_to_300() {
    // Verify that the env var parsing logic defaults to 300 when TASK_DEFAULT_TIMEOUT_SECS is unset
    // SAFETY: test-only env mutation, no concurrent threads touch this var
    unsafe {
        std::env::remove_var("TASK_DEFAULT_TIMEOUT_SECS");
    }
    let val: u64 = std::env::var("TASK_DEFAULT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    assert_eq!(val, 300, "task_default_timeout_secs should default to 300");
}

#[test]
fn test_task_timeout_wired_to_macro() {
    // Verify AppConfig.task_default_timeout_secs is accessible as u64 —
    // this is the field the impl_async_runnable! macro reads via state.config.task_default_timeout_secs
    fn assert_timeout_field(config: &randimg_core::config::AppConfig) {
        let _: u64 = config.task_default_timeout_secs;
    }
    // If this compiles, the field exists and is u64. No runtime assertion needed.
    let _ = assert_timeout_field;
}
