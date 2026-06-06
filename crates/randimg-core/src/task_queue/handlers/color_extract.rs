use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait};

use crate::WorkerState;

use super::super::jobs::*;

/// Extract color palette from a downloaded image.
///
/// The heavy computation (image decode + KMeans) is offloaded to
/// `spawn_blocking` so it runs on tokio's blocking thread pool,
/// not on the async worker threads. Combined with the dedicated
/// rayon pool inside `extract_theme_colors`, this ensures color
/// extraction never blocks the async runtime.
pub async fn handle_color_extract(
    job: ColorExtractJob,
    state: &Arc<WorkerState>,
) -> Result<(), String> {
    // Idempotency: skip if colors already extracted
    use crate::db::entities::image::Entity as Image;
    if let Some(img) = Image::find_by_id(job.image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        if img.colors.is_some() {
            tracing::debug!(
                image_id = job.image_id,
                "Image already has colors, skipping"
            );
            return Ok(());
        }
    }

    let image_dir = state.config.image_dir.clone();
    let image_id = job.image_id;
    let color_k = state.config.color_extract_k;
    let color_max_iter = state.config.color_extract_max_iter;
    let color_batch_size = state.config.color_extract_batch_size;
    let color_image_scale = state.config.color_extract_image_scale;

    let colors = tokio::task::spawn_blocking(move || {
        let full_path = format!("{}/{}", image_dir, job.image_path);
        let img = ::image::open(&full_path).map_err(|e| format!("Failed to open image: {}", e))?;
        Ok::<_, String>(crate::color::extract_theme_colors_with_config(
            &img,
            color_k,
            color_max_iter,
            color_batch_size,
            color_image_scale,
        ))
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    // DB writes stay on the async runtime (they are I/O-bound)
    use crate::db::entities::image;
    if let Some(img_model) = Image::find_by_id(image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        let txn = state
            .db
            .begin()
            .await
            .map_err(|e| format!("Failed to begin transaction: {}", e))?;

        let mut active: image::ActiveModel = img_model.into();
        active.colors = Set(Some(serde_json::to_value(&colors).unwrap()));
        active.primary_l = Set(Some(colors.primary_lab[0] as f64));
        active.primary_a = Set(Some(colors.primary_lab[1] as f64));
        active.primary_b = Set(Some(colors.primary_lab[2] as f64));
        active.update(&txn).await.map_err(|e| e.to_string())?;

        // Upsert palette entries
        use crate::db::entities::image_color_palette::{self, Entity as PaletteEntity};

        PaletteEntity::delete_many()
            .filter(image_color_palette::Column::ImageId.eq(image_id))
            .exec(&txn)
            .await
            .map_err(|e| format!("Failed to clear old palette: {}", e))?;

        for (i, (rgb, lab)) in colors
            .colors
            .iter()
            .zip(colors.colors_lab.iter())
            .enumerate()
        {
            let entry = image_color_palette::ActiveModel {
                id: sea_orm::NotSet,
                image_id: Set(image_id),
                color_index: Set(i as i32),
                rgb_r: Set(rgb[0] as i32),
                rgb_g: Set(rgb[1] as i32),
                rgb_b: Set(rgb[2] as i32),
                lab_l: Set(lab[0] as f64),
                lab_a: Set(lab[1] as f64),
                lab_b: Set(lab[2] as f64),
            };
            entry
                .insert(&txn)
                .await
                .map_err(|e| format!("Failed to insert palette entry: {}", e))?;
        }

        txn.commit()
            .await
            .map_err(|e| format!("Failed to commit palette transaction: {}", e))?;
    }

    Ok(())
}
