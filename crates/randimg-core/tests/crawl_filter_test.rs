#![cfg(feature = "http")]

use serde_json::json;

#[test]
fn test_create_crawler_request_with_filters() {
    // Verify the request DTO accepts the new fields
    let request = json!({
        "crawl_type": 0,
        "ranking_mode": "day",
        "exclude_r18": true,
        "exclude_ai": false
    });

    // Parse into the request struct
    let parsed: Result<randimg_core::handlers::crawler::CreateCrawlerRequest, _> =
        serde_json::from_value(request);

    assert!(
        parsed.is_ok(),
        "Failed to parse request with filter fields: {:?}",
        parsed.err()
    );

    let req = parsed.unwrap();
    assert_eq!(req.exclude_r18, Some(true));
    assert_eq!(req.exclude_ai, Some(false));
}

#[test]
fn test_create_crawler_request_without_filters() {
    // Verify backward compatibility - old requests without filters still work
    let request = json!({
        "crawl_type": 0,
        "ranking_mode": "day"
    });

    let parsed: Result<randimg_core::handlers::crawler::CreateCrawlerRequest, _> =
        serde_json::from_value(request);

    assert!(
        parsed.is_ok(),
        "Failed to parse request without filter fields: {:?}",
        parsed.err()
    );

    let req = parsed.unwrap();
    assert_eq!(req.exclude_r18, None);
    assert_eq!(req.exclude_ai, None);
}

#[test]
fn test_crawl_job_serialization_with_filters() {
    // Verify CrawlJob serializes/deserializes correctly with new fields
    use randimg_core::task_queue::jobs::CrawlJob;

    let job = CrawlJob {
        crawler_id: 1,
        crawl_type: 0,
        target_user_id: None,
        target_start_date: None,
        target_end_date: None,
        target_search_prompt: None,
        ranking_mode: Some("day".to_string()),
        illust_type: None,
        exclude_r18: Some(true),
        exclude_ai: Some(true),
        max_pages: None,
        discover_hops: None,
        discover_seed_limit: None,
        discover_seed_method: None,
        parent_job_id: None,
    };

    let json = serde_json::to_string(&job).unwrap();
    let deserialized: CrawlJob = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.exclude_r18, Some(true));
    assert_eq!(deserialized.exclude_ai, Some(true));
}
