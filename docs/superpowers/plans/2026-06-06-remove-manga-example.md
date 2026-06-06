# Remove Manga Example Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create an example binary that removes all manga images (illust_type = "manga") from OSS, local storage, and database, with dry run support.

**Architecture:** Single file example using WorkerState for database, OSS, and config access. Queries all manga images, then sequentially deletes each from OSS, local filesystem, and database.

**Tech Stack:** tokio, sea-orm, randimg-core (WorkerState, DogeCloudOss, AppConfig)

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `examples/remove_manga.rs` | Create | Main example binary |

## Task 1: Create remove_manga Example

**Files:**
- Create: `examples/remove_manga.rs`

- [ ] **Step 1: Create the example file with CLI argument parsing**

```rust
//! Remove all manga images (illust_type = "manga") from the system.
//!
//! Usage:
//!   cargo run --example remove_manga              # dry run (default)
//!   cargo run --example remove_manga -- --execute # actually delete

use randimg_core::config::AppConfig;
use randimg_core::db::entities::image;
use randimg_core::dogecloud::DogeCloudOss;
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait, DeleteResult};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let args: Vec<String> = std::env::args().collect();
    let dry_run = !args.contains(&"--execute".to_string());

    if dry_run {
        tracing::info!("[DRY RUN] Mode enabled. No changes will be made.");
        tracing::info!("Run with --execute to actually delete manga images.");
    }

    // Load config and connect to database
    let config = AppConfig::from_env();
    let db = sea_orm::Database::connect(&config.api_database_url).await?;
    let http_client = reqwest::Client::new();
    let oss = DogeCloudOss::new(&config, http_client);

    // Query all manga images
    let manga_images = find_manga_images(&db).await?;

    if manga_images.is_empty() {
        tracing::info!("No manga images found.");
        return Ok(());
    }

    tracing::info!("Found {} manga images to remove", manga_images.len());

    // Process each image
    let mut success_count = 0u64;
    let mut fail_count = 0u64;

    for img in &manga_images {
        tracing::info!(
            "  - Image #{}: \"{}\" (path: {})",
            img.id,
            img.title,
            img.image_path
        );

        if dry_run {
            continue;
        }

        match delete_image(&db, &oss, &config.image_dir, img).await {
            Ok(()) => {
                tracing::info!("  ✓ Deleted Image #{}", img.id);
                success_count += 1;
            }
            Err(e) => {
                tracing::error!("  ✗ Failed to delete Image #{}: {}", img.id, e);
                fail_count += 1;
            }
        }
    }

    // Summary
    if dry_run {
        tracing::info!(
            "[DRY RUN] Would delete {} images. Run with --execute to proceed.",
            manga_images.len()
        );
    } else {
        tracing::info!(
            "[EXECUTE] Complete: {} succeeded, {} failed",
            success_count,
            fail_count
        );
    }

    Ok(())
}

/// Query all images where illust_type = "manga" and deleted_at IS NULL.
async fn find_manga_images(
    db: &DatabaseConnection,
) -> Result<Vec<image::Model>, sea_orm::DbErr> {
    use image::{Column, Entity};

    Entity::find()
        .filter(Column::IllustType.eq("manga"))
        .filter(Column::DeletedAt.is_null())
        .all(db)
        .await
}

/// Delete a single image from OSS, local storage, and database.
async fn delete_image(
    db: &DatabaseConnection,
    oss: &DogeCloudOss,
    image_dir: &str,
    img: &image::Model,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Delete from OSS (ignore errors - key may not exist)
    if let Err(e) = oss.delete(&img.image_path).await {
        tracing::warn!("  OSS delete failed for {}: {}", img.image_path, e);
    }

    // 2. Delete local file (ignore if not found)
    let local_path = PathBuf::from(image_dir).join(&img.image_path);
    match tokio::fs::remove_file(&local_path).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!("  Local file not found: {}", local_path.display());
        }
        Err(e) => {
            return Err(format!("Local file delete failed: {}", e).into());
        }
    }

    // 3. Delete associated records (image_tag_association, image_color_palette)
    // These should cascade, but we delete explicitly for safety
    use randimg_core::db::entities::image_tag_association;
    use randimg_core::db::entities::image_color_palette;

    image_tag_association::Entity::delete_many()
        .filter(image_tag_association::Column::ImageId.eq(img.id))
        .exec(db)
        .await?;

    image_color_palette::Entity::delete_many()
        .filter(image_color_palette::Column::ImageId.eq(img.id))
        .exec(db)
        .await?;

    // 4. Hard delete image record
    image::Entity::delete_by_id(img.id).exec(db).await?;

    Ok(())
}
```

- [ ] **Step 2: Verify the example compiles**

Run: `cargo check --example remove_manga`
Expected: Compilation succeeds (or dependency errors to fix)

- [ ] **Step 3: Test dry run mode**

Run: `cargo run --example remove_manga`
Expected: Shows manga images found (or "No manga images found" if none exist)

- [ ] **Step 4: Commit**

```bash
git add examples/remove_manga.rs
git commit -m "feat(example): add remove_manga utility with dry run support"
```
