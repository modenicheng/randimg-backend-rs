use crate::AppState;
use crate::db::entities::task;
use crate::color::extract_theme_colors;
use sea_orm::*;

pub async fn run(state: &AppState, task: &task::Model) -> Result<(), String> {
    let image_id = task.image_id
        .or_else(|| task.payload["image_id"].as_i64().map(|v| v as i32))
        .ok_or("missing image_id in payload")? as i32;

    let image_path = task.payload["image_path"]
        .as_str()
        .ok_or("missing image_path in payload")?;

    let file_path = format!("{}/{}", state.config.image_dir, image_path);
    let img = ::image::open(&file_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let colors = extract_theme_colors(&img);

    // Update images table: primary LAB + colors JSON (backward compat)
    use crate::db::entities::image::{self, Entity as Image};
    if let Some(img_model) = Image::find_by_id(image_id)
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        let mut active: image::ActiveModel = img_model.into();
        active.colors = Set(Some(serde_json::to_value(&colors).unwrap()));
        active.primary_l = Set(Some(colors.primary_lab[0]));
        active.primary_a = Set(Some(colors.primary_lab[1]));
        active.primary_b = Set(Some(colors.primary_lab[2]));
        active.update(&state.db).await.map_err(|e| e.to_string())?;
    }

    // Upsert palette entries into image_color_palette
    use crate::db::entities::image_color_palette::{self, Entity as PaletteEntity};

    // Delete existing entries for this image (idempotent re-run)
    PaletteEntity::delete_many()
        .filter(image_color_palette::Column::ImageId.eq(image_id))
        .exec(&state.db)
        .await
        .map_err(|e| format!("Failed to clear old palette: {}", e))?;

    // Insert new entries
    for (i, (rgb, lab)) in colors.colors.iter().zip(colors.colors_lab.iter()).enumerate() {
        let entry = image_color_palette::ActiveModel {
            id: NotSet,
            image_id: Set(image_id),
            color_index: Set(i as i32),
            rgb_r: Set(rgb[0] as i32),
            rgb_g: Set(rgb[1] as i32),
            rgb_b: Set(rgb[2] as i32),
            lab_l: Set(lab[0]),
            lab_a: Set(lab[1]),
            lab_b: Set(lab[2]),
        };
        entry.insert(&state.db).await.map_err(|e| format!("Failed to insert palette entry: {}", e))?;
    }

    Ok(())
}
