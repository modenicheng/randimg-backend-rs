use std::sync::Arc;

use sea_orm::{ActiveModelTrait, EntityTrait, Set};

use crate::WorkerState;

use super::super::jobs::*;

/// Check image accessibility (stub — marks image as accessible).
pub async fn handle_accessibility_check(
    job: AccessibilityCheckJob,
    state: &Arc<WorkerState>,
) -> Result<(), String> {
    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img_model) = Image::find_by_id(job.image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        if img_model.accessible.is_some() {
            tracing::debug!(image_id = job.image_id, "Image already checked, skipping");
            return Ok(());
        }
        let mut active: image::ActiveModel = img_model.into();
        active.accessible = Set(Some(true));
        active.update(&state.db).await.map_err(|e| e.to_string())?;
    }

    Ok(())
}
