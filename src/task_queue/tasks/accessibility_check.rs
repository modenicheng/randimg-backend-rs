use crate::AppState;
use crate::db::entities::task;
use sea_orm::*;

/// Accessibility check stub - marks image as accessible.
/// Solid-color detection is planned for a future scoring model.
pub async fn run(state: &AppState, task: &task::Model) -> Result<(), String> {
    let image_id = task.payload["image_id"]
        .as_i64()
        .ok_or("missing image_id in payload")? as i32;

    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img_model) = Image::find_by_id(image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        let mut active: image::ActiveModel = img_model.into();
        active.accessable = Set(Some(true));
        active.update(&state.db).await.map_err(|e| e.to_string())?;
    }

    Ok(())
}
