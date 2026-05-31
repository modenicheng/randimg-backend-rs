pub mod kmeans;

use image::{DynamicImage, GenericImageView};
use serde::Serialize;

#[derive(Serialize)]
pub struct ThemeColors {
    pub primary_color: [u8; 3],
    pub colors: Vec<[u8; 3]>,
}

/// Extract theme colors from an image
/// Matches original Python implementation: KMeans(n_clusters=10), sort by brightness, pick middle
pub fn extract_theme_colors(img: &DynamicImage) -> ThemeColors {
    // Scale down to reduce computation
    let scale = 0.5;
    let (w, h) = img.dimensions();
    let new_w = ((w as f64 * scale) as u32).max(1);
    let new_h = ((h as f64 * scale) as u32).max(1);
    let small = img.resize_exact(new_w, new_h, image::imageops::FilterType::Triangle);
    let rgb = small.to_rgb8();

    // Collect pixels
    let pixels: Vec<[f64; 3]> = rgb
        .pixels()
        .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
        .collect();

    if pixels.is_empty() {
        return ThemeColors {
            primary_color: [0, 0, 0],
            colors: vec![[0, 0, 0]; 10],
        };
    }

    // KMeans clustering
    let centroids = kmeans::kmeans(&pixels, 10, 50);

    // Sort by brightness
    let mut sorted: Vec<[f64; 3]> = centroids;
    sorted.sort_by(|a, b| {
        let ba = brightness(a);
        let bb = brightness(b);
        ba.partial_cmp(&bb).unwrap()
    });

    // Pick middle brightness as primary color
    let mid = sorted.len() / 2;
    let primary = sorted[mid];

    ThemeColors {
        primary_color: [primary[0] as u8, primary[1] as u8, primary[2] as u8],
        colors: sorted
            .into_iter()
            .map(|c| [c[0] as u8, c[1] as u8, c[2] as u8])
            .collect(),
    }
}

fn brightness(c: &[f64; 3]) -> f64 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}
