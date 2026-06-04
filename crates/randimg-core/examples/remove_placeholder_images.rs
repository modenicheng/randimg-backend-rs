//! 使用文件哈希移除 Pixiv 占位图（不调用 API）
//!
//! 检测逻辑:
//!   计算文件 MD5 与已知占位图哈希比对
//!
//! 用法:
//!   cargo run --example remove_placeholder_images [--dry-run] [--confirm]
//!
//! 需要 .env 文件配置 API_DATABASE_URL 和 IMAGE_DIR

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use std::path::PathBuf;
use std::time::Instant;

const PLACEHOLDER_HASHES: &[&str] = &[
    "d61e3f0b610584af427d46f25052d7b6", // limit_unknown_360.png
    "f894df396e1f88333bc6022c396c3907", // limit_unviewable_360.png
    "8eec510e57f5f732fd2cce73df7b73ef", // limit_sanity_level_360.png
];
const MAX_PLACEHOLDER_SIZE: u64 = 50 * 1024;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter("remove_placeholder=info")
        .init();

    let args: Vec<String> = std::env::args().collect();
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let confirm = args.iter().any(|a| a == "--confirm");

    if dry_run {
        println!("=== DRY RUN 模式（不删除任何文件或记录）===\n");
    } else if !confirm {
        eprintln!("错误: 必须指定 --dry-run 或 --confirm");
        eprintln!("  --dry-run  仅预览，不删除");
        eprintln!("  --confirm  确认执行删除");
        std::process::exit(1);
    }

    let config = randimg_core::config::AppConfig::from_env();
    let db = sea_orm::Database::connect(&config.api_database_url)
        .await
        .expect("Failed to connect to database");

    let image_dir = PathBuf::from(&config.image_dir);

    println!("=== 移除 Pixiv 占位图 ===\n");
    println!("已知占位图哈希:");
    for h in PLACEHOLDER_HASHES {
        println!("  {h}");
    }
    println!();

    use randimg_core::db::entities::image::{self, Entity as Image};
    let images = Image::find()
        .filter(image::Column::DeletedAt.is_null())
        .filter(image::Column::SourceId.is_not_null())
        .all(&db)
        .await
        .expect("Failed to query images");

    println!("找到 {} 张未删除的图片\n", images.len());

    let mut stats = Stats::default();
    let start = Instant::now();

    for (idx, img) in images.iter().enumerate() {
        let progress = format!("[{}/{}]", idx + 1, images.len());
        let file_path = image_dir.join(&img.image_path);

        let meta = match tokio::fs::metadata(&file_path).await {
            Ok(m) => m,
            Err(_) => {
                stats.file_missing += 1;
                continue;
            }
        };

        if meta.len() > MAX_PLACEHOLDER_SIZE {
            stats.ok += 1;
            continue;
        }

        let data = match tokio::fs::read(&file_path).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{} DB#{} — 读取文件失败: {}", progress, img.id, e);
                stats.error += 1;
                continue;
            }
        };

        let hash = format!("{:x}", md5::compute(&data));

        if PLACEHOLDER_HASHES.contains(&hash.as_str()) {
            println!("{} DB#{} — 占位图 (hash={})", progress, img.id, hash);

            if !dry_run {
                if let Err(e) = tokio::fs::remove_file(&file_path).await {
                    eprintln!("  ✗ 删除文件失败: {}", e);
                    stats.error += 1;
                    continue;
                }

                use randimg_core::db::entities::image::Entity as ImageEntity;
                if let Err(e) = ImageEntity::delete_by_id(img.id).exec(&db).await {
                    eprintln!("  ✗ 删除记录失败: {}", e);
                    stats.error += 1;
                    continue;
                }
            }

            stats.placeholder += 1;
        } else {
            stats.ok += 1;
        }
    }

    println!("\n=== 完成 ===");
    if dry_run {
        println!("（DRY RUN 模式，未实际删除）");
    }
    println!("耗时: {:?}", start.elapsed());
    println!("总数: {}", images.len());
    println!("正常: {}", stats.ok);
    println!("占位图: {}", stats.placeholder);
    println!("文件缺失: {}", stats.file_missing);
    println!("错误: {}", stats.error);
}

#[derive(Default)]
struct Stats {
    ok: usize,
    placeholder: usize,
    file_missing: usize,
    error: usize,
}
