use std::sync::Arc;

use sea_orm::{ActiveModelTrait, EntityTrait, Set};

use crate::WorkerState;

use super::super::jobs::*;
use super::helpers::{mark_downloaded, spawn_downstream_children};

/// Download a single image from Pixiv to local disk.
pub async fn handle_download(job: DownloadJob, state: &Arc<WorkerState>) -> Result<(), String> {
    let current_id = job.task_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let file_path = format!("{}/{}", state.config.image_dir, job.image_path);

    // 1. Check if file already exists on disk
    if tokio::fs::metadata(&file_path).await.is_ok() {
        tracing::info!("File already exists on disk, skipping download: {}", job.image_path);
        if let Err(e) = mark_downloaded(&state.db, job.image_id).await {
            tracing::warn!(image_id = job.image_id, "Failed to mark as downloaded: {}", e);
        }

        // ┌─────────────────────────────────────────────────────────────────────┐
        // │ 校验已存在文件：读取实际像素尺寸并回写数据库                          │
        // │                                                                     │
        // │ 为什么：save_illust() 因 Pixiv API 不返回各页独立尺寸，只能写入 0。  │
        // │ 此处对已存在文件补全真实宽高，确保数据库数据准确。                    │
        // │                                                                     │
        // │ 错误处理：若文件损坏无法解码，删除文件并返回错误，触发 Apalis 重试。  │
        // └─────────────────────────────────────────────────────────────────────┘
        {
            use crate::db::entities::image::{self, Entity as Image};
            let fp = file_path.clone();
            let image_id = job.image_id;

            let dimensions = match tokio::task::spawn_blocking(move || {
                let img = ::image::open(&fp)
                    .map_err(|e| format!("Failed to open/validate image: {}", e))?;
                let (w, h) = (img.width() as i32, img.height() as i32);
                Ok::<_, String>((w, h))
            })
            .await
            .map_err(|e| format!("spawn_blocking panicked: {}", e))?
            {
                Ok(dims) => dims,
                Err(e) => {
                    tracing::error!(image_id = job.image_id, "Image file corrupt/invalid: {}", e);
                    if let Err(rm_err) = tokio::fs::remove_file(&file_path).await {
                        tracing::warn!(path = %file_path, error = %rm_err, "Failed to remove corrupt file");
                    }
                    return Err(e.into());
                }
            };

            let (w, h) = dimensions;
            let ar = if h > 0 { w as f32 / h as f32 } else { 0.0 };

            if let Some(img_model) = Image::find_by_id(image_id)
                .one(&state.db)
                .await
                .map_err(|e| e.to_string())?
            {
                let mut active: image::ActiveModel = img_model.into();
                active.width = Set(w);
                active.height = Set(h);
                active.aspect_ratio = Set(ar);
                active.update(&state.db).await.map_err(|e| e.to_string())?;
            }
        }

        spawn_downstream_children(&job, &current_id, &**state).await?;
        return Ok(());
    }

    // 2. Check DB downloaded flag — if true but file is missing, log warning and re-download
    {
        use crate::db::entities::image::Entity as Image;
        if let Some(img) = Image::find_by_id(job.image_id).one(&state.db).await.map_err(|e| e.to_string())? {
            if img.downloaded {
                tracing::warn!(
                    image_id = job.image_id,
                    "DB says downloaded but file missing, re-downloading: {}",
                    job.image_path
                );
            }
        }
    }

    // 3. Perform the actual download using crate's built-in downloader.
    {
        use std::path::Path;
        let img_path = Path::new(&job.image_path);
        let parent = img_path.parent().unwrap_or(Path::new(""));
        let filename = img_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Invalid image_path: {}", job.image_path))?;

        let output_dir = if parent.as_os_str().is_empty() {
            Path::new(&state.config.image_dir).to_path_buf()
        } else {
            let dir = Path::new(&state.config.image_dir).join(parent);
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(|e| format!("Failed to create directory: {}", e))?;
            dir
        };

        let dm = crate::pixiv::downloader::DownloadManager::new(
            state.http_client.clone(),
            &output_dir,
        );
        dm.download(&job.source_image_url, filename)
            .await
            .map_err(|e| format!("Download failed: {}", e))?;
    }

    tracing::info!("Downloaded image {} to {}", job.image_id, file_path);

    // 4. Mark as downloaded in DB
    if let Err(e) = mark_downloaded(&state.db, job.image_id).await {
        tracing::warn!(image_id = job.image_id, "Failed to mark as downloaded: {}", e);
    }

    validate_and_update_dimensions(&state.db, &file_path, job.image_id).await?;

    // 5. Spawn downstream pipeline tasks
    spawn_downstream_children(&job, &current_id, &**state).await?;

    Ok(())
}

async fn validate_and_update_dimensions(
    db: &sea_orm::DatabaseConnection,
    file_path: &str,
    image_id: i32,
) -> Result<(), String> {
    use crate::db::entities::image::{self, Entity as Image};
    let fp = file_path.to_string();
    let fp_clone = fp.clone();
    let id = image_id;

    let dimensions = tokio::task::spawn_blocking(move || {
        let img = ::image::open(&fp)
            .map_err(|e| format!("Failed to open/validate image: {}", e))?;
        let (w, h) = (img.width() as i32, img.height() as i32);
        Ok::<_, String>((w, h))
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))?
    .map_err(|e| {
        let fp = fp_clone;
        tokio::task::spawn(async move {
            if let Err(rm_err) = tokio::fs::remove_file(&fp).await {
                tracing::warn!(path = %fp, error = %rm_err, "Failed to remove corrupt file");
            }
        });
        e
    })?;

    let (w, h) = dimensions;
    let ar = if h > 0 { w as f32 / h as f32 } else { 0.0 };

    if let Some(img_model) = Image::find_by_id(id)
        .one(db)
        .await
        .map_err(|e| e.to_string())?
    {
        let mut active: image::ActiveModel = img_model.into();
        active.width = Set(w);
        active.height = Set(h);
        active.aspect_ratio = Set(ar);
        active.update(db).await.map_err(|e| e.to_string())?;
    }

    Ok(())
}
