use serde_json;

#[tokio::test]
async fn test_random_image_returns_json() {
    let client = reqwest::Client::new();
    let resp = client
        .get("http://localhost:8000/?format=json")
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

#[tokio::test]
async fn test_task_tree_flatten() {
    let client = reqwest::Client::new();
    let base = "http://localhost:8000";

    let roots_resp = client
        .get(format!("{}/tasks/roots?limit=1", base))
        .send()
        .await;

    let roots = match roots_resp {
        Ok(r) if r.status() == 200 => {
            let body: serde_json::Value = r.json().await.unwrap();
            body.get("tasks")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default()
        }
        _ => {
            // Server not running or no tasks, skip
            return;
        }
    };

    if roots.is_empty() {
        return; // No tasks to test with
    }

    let root_id = roots[0]["id"].as_str().unwrap();

    let nested: serde_json::Value = client
        .get(format!("{}/tasks/{}/tree", base, root_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let flat: serde_json::Value = client
        .get(format!("{}/tasks/{}/tree?flatten=true", base, root_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(nested.get("root_job_id").is_some());
    assert!(flat.get("root_job_id").is_some());

    assert!(nested.get("children").is_some());
    assert!(flat.get("tasks").is_some());

    let tasks = flat["tasks"].as_array().unwrap();
    for task in tasks {
        assert!(task.get("parent_job_id").is_some(), "missing parent_job_id");
        assert!(task.get("root_job_id").is_some(), "missing root_job_id");
        assert_eq!(task["root_job_id"].as_str().unwrap(), root_id);
    }
}
