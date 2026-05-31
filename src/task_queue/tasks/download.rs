use crate::AppState;
use crate::db::entities::task;

pub async fn run(state: &AppState, task: &task::Model) -> Result<(), String> {
    let image_id = task.image_id
        .or_else(|| task.payload["image_id"].as_i64().map(|v| v as i32))
        .ok_or("missing image_id")?;

    let source_image_url = task.payload["source_image_url"]
        .as_str()
        .ok_or("missing source_image_url")?;

    let image_path = task.payload["image_path"]
        .as_str()
        .ok_or("missing image_path")?;

    // Download the image using reqwest with Pixiv referer
    let client = reqwest::Client::new();
    let resp = client
        .get(source_image_url)
        .header("Referer", "https://app-api.pixiv.net/")
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Download failed with status: {}", resp.status()));
    }

    let bytes = resp.bytes().await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // Save to image_dir
    let file_path = format!("{}/{}", state.config.image_dir, image_path);

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&file_path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    tokio::fs::write(&file_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write file: {}", e))?;

    tracing::info!("Downloaded image {} to {}", image_id, file_path);
    Ok(())
}
