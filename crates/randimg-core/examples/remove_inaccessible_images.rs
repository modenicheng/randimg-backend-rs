//! 移除所有未删除的 Pixiv 图片（数据库记录 + 本地文件）
//!
//! 用法:
//!   cargo run --example remove_inaccessible_images [--dry-run]
//!
//! 需要 .env 文件配置 API_DATABASE_URL

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use std::path::PathBuf;
use std::time::Instant;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter("remove_inaccessible=info")
        .init();

    let dry_run = std::env::args().any(|a| a == "--dry-run");
    if dry_run {
        println!("=== DRY RUN 模式（不删除任何文件或记录）===\n");
    }

    let config = randimg_core::config::AppConfig::from_env();
    let db = sea_orm::Database::connect(&config.api_database_url)
        .await
        .expect("Failed to connect to database");

    let image_dir = PathBuf::from(&config.image_dir);

    println!("=== 移除所有未删除的 Pixiv 图片 ===\n");

    use randimg_core::db::entities::image::{self, Entity as Image};
    let images = Image::find()
        .filter(image::Column::DeletedAt.is_null())
        .filter(image::Column::SourceId.is_not_null())
        .all(&db)
        .await
        .expect("Failed to query images");

    println!("找到 {} 张未删除的图片\n", images.len());

    let mut files_deleted = 0usize;
    let mut files_not_found = 0usize;
    let mut db_deleted = 0usize;
    let start = Instant::now();

    for (idx, img) in images.iter().enumerate() {
        let progress = format!("[{}/{}]", idx + 1, images.len());
        let file_path = image_dir.join(&img.image_path);

        if dry_run {
            println!("{} DB#{} {} — 会删除", progress, img.id, file_path.display());
            continue;
        }

        if file_path.exists() {
            match tokio::fs::remove_file(&file_path).await {
                Ok(_) => {
                    println!("{} DB#{} — 已删除文件", progress, img.id);
                    files_deleted += 1;
                }
                Err(e) => {
                    eprintln!("{} DB#{} — 删除文件失败: {}", progress, img.id, e);
                }
            }
        } else {
            files_not_found += 1;
        }

        use randimg_core::db::entities::image::Entity as ImageEntity;
        if let Err(e) = ImageEntity::delete_by_id(img.id).exec(&db).await {
            eprintln!("{} DB#{} — 删除记录失败: {}", progress, img.id, e);
        } else {
            db_deleted += 1;
        }
    }

    println!("\n=== 完成 ===");
    if dry_run {
        println!("（DRY RUN 模式，未实际删除）");
    }
    println!("耗时: {:?}", start.elapsed());
    println!("总数: {}", images.len());
    println!("文件已删除: {}", files_deleted);
    println!("文件不存在: {}", files_not_found);
    println!("记录已删除: {}", db_deleted);
}
