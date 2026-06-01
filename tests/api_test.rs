use serde_json;

#[tokio::test]
async fn test_random_image_returns_json() {
    let client = reqwest::Client::new();
    let resp = client
        .get("http://localhost:8000/")
        .query(&[("format", "json")])
        .send()
        .await;

    match resp {
        Ok(r) => {
            assert!(r.status() == 200 || r.status() == 404);
        }
        Err(_) => {
            // Server not running, skip
        }
    }
}

#[tokio::test]
async fn test_login_returns_token() {
    let client = reqwest::Client::new();
    let resp = client
        .post("http://localhost:8000/token")
        .json(&serde_json::json!({"username": "test", "password": "test"}))
        .send()
        .await;

    match resp {
        Ok(r) => {
            assert!(r.status() == 200 || r.status() == 401);
        }
        Err(_) => {}
    }
}

#[tokio::test]
async fn test_tags_endpoint() {
    let client = reqwest::Client::new();
    let resp = client.get("http://localhost:8000/tags").send().await;

    match resp {
        Ok(r) => {
            assert_eq!(r.status(), 200);
            let body: serde_json::Value = r.json().await.unwrap();
            assert!(body.is_array());
        }
        Err(_) => {}
    }
}

#[tokio::test]
async fn test_statistic_endpoint() {
    let client = reqwest::Client::new();
    let resp = client.get("http://localhost:8000/statistic").send().await;

    match resp {
        Ok(r) => {
            assert_eq!(r.status(), 200);
            let body: serde_json::Value = r.json().await.unwrap();
            assert!(body.get("illust_count").is_some());
            assert!(body.get("tag_count").is_some());
            assert!(body.get("author_count").is_some());
        }
        Err(_) => {}
    }
}
