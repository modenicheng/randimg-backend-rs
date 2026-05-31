use crate::AppState;
use crate::db::entities::task;
use sea_orm::*;

pub async fn run(state: &AppState, task: &task::Model) -> Result<(), String> {
    let image_id = task.image_id
        .or_else(|| task.payload["image_id"].as_i64().map(|v| v as i32))
        .ok_or("missing image_id")?;

    let image_path = task.image_path
        .as_deref()
        .or_else(|| task.payload["image_path"].as_str())
        .ok_or("missing image_path")?;

    // 读取本地文件
    let file_path = format!("{}/{}", state.config.image_dir, image_path);
    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|e| format!("Failed to read image file {}: {}", file_path, e))?;

    // 上传到 DogeCloud OSS
    state.oss
        .upload(image_path, bytes)
        .await
        .map_err(|e| format!("OSS upload failed: {}", e))?;

    tracing::info!(
        image_id = image_id,
        path = %image_path,
        "Uploaded image to DogeCloud OSS"
    );

    // Mark image as public: pipeline completed (download + color_extract + upload)
    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img_model) = Image::find_by_id(image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        let mut active: image::ActiveModel = img_model.into();
        active.is_public = Set(true);
        active.update(&state.db).await.map_err(|e| e.to_string())?;
    }

    Ok(())
}
