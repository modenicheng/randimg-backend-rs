//! Unit tests for task queue job structs: serialization/deserialization.

use randimg_backend_rs::task_queue::jobs::*;

#[test]
fn test_crawl_job_roundtrip() {
    let job = CrawlJob {
        crawler_id: 1,
        crawl_type: 1,
        target_user_id: Some("12345".into()),
        target_start_date: None,
        target_end_date: None,
        target_search_prompt: Some("landscape".into()),
        parent_job_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: CrawlJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.crawler_id, 1);
    assert_eq!(deserialized.crawl_type, 1);
    assert_eq!(deserialized.target_user_id.as_deref(), Some("12345"));
    assert_eq!(deserialized.target_search_prompt.as_deref(), Some("landscape"));
}

#[test]
fn test_download_job_roundtrip() {
    let job = DownloadJob {
        image_id: 42,
        source_image_url: "https://example.com/image.jpg".into(),
        image_path: "/data/images/42.jpg".into(),
        parent_job_id: None,
        root_job_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: DownloadJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 42);
    assert_eq!(deserialized.source_image_url, "https://example.com/image.jpg");
    assert_eq!(deserialized.image_path, "/data/images/42.jpg");
}

#[test]
fn test_color_extract_job_roundtrip() {
    let job = ColorExtractJob {
        image_id: 10,
        image_path: "/data/images/10.jpg".into(),
        parent_job_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: ColorExtractJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 10);
    assert_eq!(deserialized.image_path, "/data/images/10.jpg");
}

#[test]
fn test_upload_job_roundtrip() {
    let job = UploadJob {
        image_id: 5,
        image_path: "/data/images/5.jpg".into(),
        parent_job_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: UploadJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 5);
}

#[test]
fn test_accessibility_check_job_roundtrip() {
    let job = AccessibilityCheckJob {
        image_id: 7,
        image_path: "/data/images/7.jpg".into(),
        parent_job_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: AccessibilityCheckJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_id, 7);
}

#[test]
fn test_discover_job_roundtrip() {
    let job = DiscoverJob {
        hop: 0,
        max_hops: Some(3),
        seed_limit: Some(10),
        seed_method: Some("popularity".into()),
        parent_job_id: Some("parent-uuid-123".into()),
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: DiscoverJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.hop, 0);
    assert_eq!(deserialized.max_hops, Some(3));
    assert_eq!(deserialized.seed_limit, Some(10));
    assert_eq!(deserialized.seed_method.as_deref(), Some("popularity"));
}

#[test]
fn test_discover_job_optional_fields_none() {
    let job = DiscoverJob {
        hop: 2,
        max_hops: None,
        seed_limit: None,
        seed_method: None,
        parent_job_id: None,
    };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: DiscoverJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.hop, 2);
    assert!(deserialized.max_hops.is_none());
    assert!(deserialized.seed_limit.is_none());
    assert!(deserialized.seed_method.is_none());
}

#[test]
fn test_refresh_pixiv_token_job_roundtrip() {
    let job = RefreshPixivTokenJob { credential_id: 99, parent_job_id: None };
    let json = serde_json::to_string(&job).unwrap();
    let deserialized: RefreshPixivTokenJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.credential_id, 99);
}

#[test]
fn test_crawl_job_deserialize_from_json_literal() {
    let json = r#"{
        "crawler_id": 10,
        "crawl_type": 0,
        "target_user_id": null,
        "target_start_date": "2026-01-01",
        "target_end_date": "2026-01-31",
        "target_search_prompt": null
    }"#;
    let job: CrawlJob = serde_json::from_str(json).unwrap();
    assert_eq!(job.crawler_id, 10);
    assert!(job.target_user_id.is_none());
    assert_eq!(job.target_start_date.as_deref(), Some("2026-01-01"));
}

#[test]
fn test_job_structs_are_clone() {
    let job = DownloadJob {
        image_id: 1,
        source_image_url: "url".into(),
        image_path: "path".into(),
        parent_job_id: None,
        root_job_id: None,
    };
    let cloned = job.clone();
    assert_eq!(cloned.image_id, 1);
}

/// Verify backward compatibility: JSON without parent_job_id still works
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
}

/// Verify parent_job_id is serialized and deserialized correctly
#[test]
fn test_parent_job_id_roundtrip() {
    let job = CrawlJob {
        crawler_id: 1,
        crawl_type: 0,
        target_user_id: None,
        target_start_date: None,
        target_end_date: None,
        target_search_prompt: None,
        parent_job_id: Some("parent-uuid-abc".into()),
    };
    let json = serde_json::to_string(&job).unwrap();
    assert!(json.contains("parent-uuid-abc"));
    let deserialized: CrawlJob = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.parent_job_id.as_deref(), Some("parent-uuid-abc"));
}
