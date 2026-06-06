use std::sync::Arc;

use sea_orm::{ActiveModelTrait, EntityTrait, Set};

use crate::WorkerState;

use super::super::jobs::*;

/// Upload a downloaded image to DogeCloud OSS.
pub async fn handle_upload(job: UploadJob, state: &Arc<WorkerState>) -> Result<(), String> {
    // Idempotency: skip if already uploaded (is_public set by previous upload)
    use crate::db::entities::image::Entity as Image;
    if let Some(img) = Image::find_by_id(job.image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        if img.is_public {
            tracing::debug!(image_id = job.image_id, "Image already uploaded, skipping");
            return Ok(());
        }
    }

    let file_path = format!("{}/{}", state.config.image_dir, job.image_path);
    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|e| format!("Failed to read image file {}: {}", file_path, e))?;

    state
        .oss
        .upload(&job.image_path, bytes)
        .await
        .map_err(|e| format!("OSS upload failed: {}", e))?;

    tracing::info!(
        image_id = job.image_id,
        path = %job.image_path,
        "Uploaded image to DogeCloud OSS"
    );

    // Mark image as public
    use crate::db::entities::image;
    if let Some(img_model) = Image::find_by_id(job.image_id)
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
