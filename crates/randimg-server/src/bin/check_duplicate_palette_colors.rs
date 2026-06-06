use std::collections::HashMap;

use migration::MigratorTrait;
use randimg_core::config::AppConfig;
use sea_orm::{Database, EntityTrait};
use tracing_subscriber::EnvFilter;

use randimg_core::db::entities::image_color_palette::{self, Entity as PaletteEntity};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = AppConfig::from_env();

    let db = Database::connect(&config.api_database_url)
        .await
        .expect("Failed to connect to database");

    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");

    tracing::info!("Checking for duplicate palette colors...");

    // Fetch all palette entries
    let all_palettes = PaletteEntity::find()
        .all(&db)
        .await
        .expect("Failed to query palette entries");

    tracing::info!(
        total_entries = all_palettes.len(),
        "Fetched palette entries"
    );

    // Group by image_id
    let mut by_image: HashMap<i32, Vec<&image_color_palette::Model>> = HashMap::new();
    for entry in &all_palettes {
        by_image.entry(entry.image_id).or_default().push(entry);
    }

    tracing::info!(image_count = by_image.len(), "Grouped by image");

    // Check each image for duplicate RGB values
    let mut images_with_duplicates: Vec<(i32, Vec<(i32, i32, i32, Vec<i32>)>)> = Vec::new();
    let mut total_images_checked = 0;
    let mut total_images_with_dupes = 0;

    for (image_id, entries) in &by_image {
        total_images_checked += 1;

        // Group palette entries by (rgb_r, rgb_g, rgb_b)
        let mut rgb_groups: HashMap<(i32, i32, i32), Vec<i32>> = HashMap::new();
        for entry in entries {
            let key = (entry.rgb_r, entry.rgb_g, entry.rgb_b);
            rgb_groups.entry(key).or_default().push(entry.color_index);
        }

        // Find duplicates (groups with more than 1 entry)
        let duplicates: Vec<(i32, i32, i32, Vec<i32>)> = rgb_groups
            .into_iter()
            .filter(|(_, indices)| indices.len() > 1)
            .map(|((r, g, b), indices)| (r, g, b, indices))
            .collect();

        if !duplicates.is_empty() {
            total_images_with_dupes += 1;
            images_with_duplicates.push((*image_id, duplicates));
        }
    }

    // Report results
    println!("=== Palette Duplicate Color Check ===");
    println!("Total images with palettes: {}", total_images_checked);
    println!("Images with duplicate colors: {}", total_images_with_dupes);
    println!();

    if images_with_duplicates.is_empty() {
        println!("No duplicate colors found in any image palette.");
    } else {
        // Sort by image_id for consistent output
        images_with_duplicates.sort_by_key(|(id, _)| *id);

        for (image_id, duplicates) in &images_with_duplicates {
            println!(
                "Image ID {}: {} duplicate color(s)",
                image_id,
                duplicates.len()
            );
            for (r, g, b, indices) in duplicates {
                let mut sorted_indices = indices.clone();
                sorted_indices.sort();
                println!(
                    "  RGB({}, {}, {}) appears at color_index: {:?}",
                    r, g, b, sorted_indices
                );
            }
        }

        println!();
        println!(
            "Summary: {} out of {} images have duplicate palette colors.",
            total_images_with_dupes, total_images_checked
        );
    }
}
