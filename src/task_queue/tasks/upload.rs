use crate::AppState;
use crate::db::entities::task;

pub async fn run(_state: &AppState, task: &task::Model) -> Result<(), String> {
    let _image_path = task.payload["image_path"]
        .as_str()
        .ok_or("missing image_path")?;

    // TODO: Implement S3 upload logic (DogeCloud OSS)
    // After upload, update uploaded=true

    Ok(())
}
