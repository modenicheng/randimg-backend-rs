use crate::AppState;
use crate::db::entities::task;

pub async fn run(_state: &AppState, task: &task::Model) -> Result<(), String> {
    let _image_id = task.payload["image_id"]
        .as_i64()
        .ok_or("missing image_id")?;

    let _source_url = task.payload["source_url"]
        .as_str()
        .ok_or("missing source_url")?;

    // TODO: Implement actual download logic (reqwest + Pixiv referer)
    // After download, update downloaded=true

    Ok(())
}
