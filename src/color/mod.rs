pub mod kmeans;

use image::{DynamicImage, GenericImageView};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct ThemeColors {
    pub primary_color: [u8; 3],
    pub colors: Vec<[u8; 3]>,
}

/// Quantize a channel value into bins (e.g., 16 levels)
fn quantize(val: u8, levels: u8) -> u8 {
    let step = 256 / levels as u16;
    ((val as u16 / step) * step + step / 2).min(255) as u8
}

/// Quantize RGB into a bin key
fn quantize_pixel(r: u8, g: u8, b: u8, levels: u8) -> [u8; 3] {
    [quantize(r, levels), quantize(g, levels), quantize(b, levels)]
}

/// Extract primary color using histogram (mode of quantized colors)
fn histogram_primary_color(pixels: &[[u8; 3]], levels: u8) -> [u8; 3] {
    let mut hist: HashMap<[u8; 3], u32> = HashMap::new();

    for p in pixels {
        let key = quantize_pixel(p[0], p[1], p[2], levels);
        *hist.entry(key).or_insert(0) += 1;
    }

    // Find the most frequent bin
    hist.into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(color, _)| color)
        .unwrap_or([0, 0, 0])
}

/// Extract top N colors by histogram frequency
fn histogram_top_colors(pixels: &[[u8; 3]], levels: u8, n: usize) -> Vec<[u8; 3]> {
    let mut hist: HashMap<[u8; 3], u32> = HashMap::new();

    for p in pixels {
        let key = quantize_pixel(p[0], p[1], p[2], levels);
        *hist.entry(key).or_insert(0) += 1;
    }

    let mut entries: Vec<([u8; 3], u32)> = hist.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by frequency descending

    entries.into_iter().take(n).map(|(c, _)| c).collect()
}

/// Extract theme colors from an image
/// Primary color: histogram mode (most frequent quantized color)
/// Color palette: KMeans clustering, sorted by brightness
pub fn extract_theme_colors(img: &DynamicImage) -> ThemeColors {
    // Scale down to reduce computation
    let scale = 0.5;
    let (w, h) = img.dimensions();
    let new_w = ((w as f64 * scale) as u32).max(1);
    let new_h = ((h as f64 * scale) as u32).max(1);
    let small = img.resize_exact(new_w, new_h, image::imageops::FilterType::Triangle);
    let rgb = small.to_rgb8();

    // Collect pixels as [u8; 3]
    let pixels: Vec<[u8; 3]> = rgb
        .pixels()
        .map(|p| [p[0], p[1], p[2]])
        .collect();

    if pixels.is_empty() {
        return ThemeColors {
            primary_color: [0, 0, 0],
            colors: vec![[0, 0, 0]; 10],
        };
    }

    // Primary color from histogram (16 levels per channel)
    let primary_color = histogram_primary_color(&pixels, 16);

    // Color palette from KMeans (10 clusters), sorted by brightness
    let pixels_f64: Vec<[f64; 3]> = pixels
        .iter()
        .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
        .collect();

    let centroids = kmeans::kmeans(&pixels_f64, 10, 50);

    let mut sorted: Vec<[f64; 3]> = centroids;
    sorted.sort_by(|a, b| {
        let ba = brightness(a);
        let bb = brightness(b);
        ba.partial_cmp(&bb).unwrap()
    });

    ThemeColors {
        primary_color,
        colors: sorted
            .into_iter()
            .map(|c| [c[0] as u8, c[1] as u8, c[2] as u8])
            .collect(),
    }
}

fn brightness(c: &[f64; 3]) -> f64 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}
