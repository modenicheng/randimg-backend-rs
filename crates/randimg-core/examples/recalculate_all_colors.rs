//! 批量重新计算所有图片的颜色与调色盘
//!
//! 用途：更新颜色提取算法后，重新计算并入库。
//! 直接操作数据库，不依赖任务队列。
//!
//! 用法:
//!   cargo run -p randimg-core --example recalculate_all_colors
//!
//! 环境变量:
//!   API_DATABASE_URL  — PostgreSQL 连接串
//!   IMAGE_DIR         — 图片目录（默认 ./images）
//!   CONCURRENCY       — 并发数（默认 6）

use randimg_core::color::{self, ThemeColors};
use randimg_core::db::entities::image::{self, Entity as Image};
use randimg_core::db::entities::image_color_palette::{self, Entity as PaletteEntity};
use sea_orm::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

#[tokio::main]
async fn main() {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter("recalculate_all_colors=info,randimg_core=info")
        .init();

    dotenvy::dotenv().ok();

    let db_url = std::env::var("API_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/randimg".into());
    let image_dir =
        std::env::var("IMAGE_DIR").unwrap_or_else(|_| "./images".into());
    let concurrency: usize = std::env::var("CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);

    tracing::info!(db_url, image_dir, concurrency, "Starting color recalculation");

    // 连接数据库
    let db = sea_orm::Database::connect(&db_url)
        .await
        .expect("Failed to connect to database");

    // 查询所有未删除、已下载的图片
    let images = Image::find()
        .filter(image::Column::DeletedAt.is_null())
        .filter(image::Column::Downloaded.eq(true))
        .all(&db)
        .await
        .expect("Failed to query images");

    let total = images.len();
    tracing::info!(total, "Found images to process");

    if total == 0 {
        tracing::info!("No images to process, exiting");
        return;
    }

    // 计数器
    let success = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let processed = Arc::new(AtomicUsize::new(0));

    // 并发信号量
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let start = Instant::now();

    // 并行处理
    let mut handles = Vec::with_capacity(total);

    for img in images {
        let db = db.clone();
        let image_dir = image_dir.clone();
        let semaphore = semaphore.clone();
        let success = success.clone();
        let failed = failed.clone();
        let skipped = skipped.clone();
        let processed = processed.clone();

        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            let image_path = PathBuf::from(&image_dir).join(&img.image_path);

            // 检查文件是否存在
            if !image_path.exists() {
                tracing::warn!(
                    image_id = img.id,
                    path = %image_path.display(),
                    "Image file not found, skipping"
                );
                skipped.fetch_add(1, Ordering::Relaxed);
                processed.fetch_add(1, Ordering::Relaxed);
                return;
            }

            // 加载图片
            let img_data = match ::image::open(&image_path) {
                Ok(d) => d,
                Err(e) => {
                    tracing::error!(
                        image_id = img.id,
                        path = %image_path.display(),
                        error = %e,
                        "Failed to open image"
                    );
                    failed.fetch_add(1, Ordering::Relaxed);
                    processed.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };

            // 提取颜色（CPU 密集，使用专用线程池）
            let colors = color::run_on_color_pool(|| {
                color::extract_theme_colors(&img_data)
            });

            // 写入数据库
            if let Err(e) = update_colors_in_db(&db, img.id, &colors).await {
                tracing::error!(
                    image_id = img.id,
                    error = %e,
                    "Failed to update database"
                );
                failed.fetch_add(1, Ordering::Relaxed);
                processed.fetch_add(1, Ordering::Relaxed);
                return;
            }

            success.fetch_add(1, Ordering::Relaxed);
            let count = processed.fetch_add(1, Ordering::Relaxed) + 1;

            // 每 10 张或最后一张输出进度
            if count % 10 == 0 || count == total {
                tracing::info!(
                    progress = format!("{}/{}", count, total),
                    success = success.load(Ordering::Relaxed),
                    failed = failed.load(Ordering::Relaxed),
                    skipped = skipped.load(Ordering::Relaxed),
                    "Progress"
                );
            }
        });

        handles.push(handle);
    }

    // 等待所有任务完成
    for handle in handles {
        let _ = handle.await;
    }

    let elapsed = start.elapsed();
    let success = success.load(Ordering::Relaxed);
    let failed = failed.load(Ordering::Relaxed);
    let skipped = skipped.load(Ordering::Relaxed);

    tracing::info!(
        total,
        success,
        failed,
        skipped,
        elapsed_secs = elapsed.as_secs_f64(),
        "Color recalculation complete"
    );

    println!("\n=== 完成 ===");
    println!("  总数:   {total}");
    println!("  成功:   {success}");
    println!("  失败:   {failed}");
    println!("  跳过:   {skipped}");
    println!("  耗时:   {:.2}s", elapsed.as_secs_f64());
    println!(
        "  速度:   {:.1} img/s",
        success as f64 / elapsed.as_secs_f64()
    );
}

/// 更新数据库中的颜色数据
async fn update_colors_in_db(
    db: &DatabaseConnection,
    image_id: i32,
    colors: &ThemeColors,
) -> Result<(), DbErr> {
    // 更新 images 表
    if let Some(img_model) = Image::find_by_id(image_id).one(db).await? {
        let mut active: image::ActiveModel = img_model.into();
        active.colors = Set(Some(serde_json::to_value(colors).unwrap()));
        active.primary_l = Set(Some(colors.primary_lab[0] as f64));
        active.primary_a = Set(Some(colors.primary_lab[1] as f64));
        active.primary_b = Set(Some(colors.primary_lab[2] as f64));
        active.update(db).await?;
    }

    // 删除旧调色盘
    PaletteEntity::delete_many()
        .filter(image_color_palette::Column::ImageId.eq(image_id))
        .exec(db)
        .await?;

    // 插入新调色盘
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
        entry.insert(db).await?;
    }

    Ok(())
}
