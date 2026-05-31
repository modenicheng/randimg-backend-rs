use crate::AppState;
use crate::db::entities::task;
use crate::color::extract_theme_colors;
use sea_orm::*;

pub async fn run(state: &AppState, task: &task::Model) -> Result<(), String> {
    let image_id = task.payload["image_id"]
        .as_i64()
        .ok_or("missing image_id in payload")? as i32;

    let image_path = task.payload["image_path"]
        .as_str()
        .ok_or("missing image_path in payload")?;

    let file_path = format!("{}/{}", state.config.image_dir, image_path);
    let img = ::image::open(&file_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let colors = extract_theme_colors(&img);

    // Update database
    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img_model) = Image::find_by_id(image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        let mut active: image::ActiveModel = img_model.into();
        active.colors = Set(Some(serde_json::to_value(&colors).unwrap()));
        active.processed = Set(true);
        active.processing = Set(false);
        active.update(&state.db).await.map_err(|e| e.to_string())?;
    }

    Ok(())
}
