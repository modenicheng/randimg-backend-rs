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

    hist.into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(color, _)| color)
        .unwrap_or([0, 0, 0])
}

// ---- RGB <-> CIELAB conversion ----

/// Linearize sRGB channel (gamma expansion)
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Apply sRGB gamma (gamma compression)
fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Convert sRGB [0,255] to CIELAB [L, a, b]
fn rgb_to_lab(r: u8, g: u8, b: u8) -> [f64; 3] {
    // sRGB -> linear RGB
    let r_lin = srgb_to_linear(r as f64 / 255.0);
    let g_lin = srgb_to_linear(g as f64 / 255.0);
    let b_lin = srgb_to_linear(b as f64 / 255.0);

    // linear RGB -> XYZ (D65 illuminant)
    let x = r_lin * 0.4124564 + g_lin * 0.3575761 + b_lin * 0.1804375;
    let y = r_lin * 0.2126729 + g_lin * 0.7151522 + b_lin * 0.0721750;
    let z = r_lin * 0.0193339 + g_lin * 0.1191920 + b_lin * 0.9503041;

    // XYZ -> LAB (D65 reference white)
    let x_ref = 0.95047;
    let y_ref = 1.0;
    let z_ref = 1.08883;

    let f = |t: f64| -> f64 {
        const DELTA: f64 = 6.0 / 29.0;
        if t > DELTA.powi(3) {
            t.powf(1.0 / 3.0)
        } else {
            t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
        }
    };

    let fx = f(x / x_ref);
    let fy = f(y / y_ref);
    let fz = f(z / z_ref);

    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let b_val = 200.0 * (fy - fz);

    [l, a, b_val]
}

/// Convert CIELAB to sRGB [0,255]
fn lab_to_rgb(l: f64, a: f64, b: f64) -> [u8; 3] {
    let x_ref = 0.95047;
    let y_ref = 1.0;
    let z_ref = 1.08883;

    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;

    let finv = |t: f64| -> f64 {
        const DELTA: f64 = 6.0 / 29.0;
        if t > DELTA {
            t * t * t
        } else {
            3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
        }
    };

    let x = finv(fx) * x_ref;
    let y = finv(fy) * y_ref;
    let z = finv(fz) * z_ref;

    // XYZ -> linear RGB
    let r_lin = x * 3.2404542 + y * -1.5371385 + z * -0.4985314;
    let g_lin = x * -0.9692660 + y * 1.8760108 + z * 0.0415560;
    let b_lin = x * 0.0556434 + y * -0.2040259 + z * 1.0572252;

    // Clamp and apply gamma
    let clamp = |v: f64| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round().min(255.0).max(0.0) as u8;

    [clamp(r_lin), clamp(g_lin), clamp(b_lin)]
}

/// Extract theme colors from an image
/// Primary color: histogram mode (most frequent quantized color)
/// Color palette: KMeans in CIELAB space, sorted by L*
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

    // Convert to LAB for clustering
    let lab_pixels: Vec<[f64; 3]> = pixels
        .iter()
        .map(|p| rgb_to_lab(p[0], p[1], p[2]))
        .collect();

    // KMeans clustering in LAB space
    let lab_centroids = kmeans::kmeans(&lab_pixels, 10, 50);

    // Sort by L* (lightness)
    let mut sorted_lab = lab_centroids;
    sorted_lab.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());

    // Convert LAB centroids back to RGB
    let colors: Vec<[u8; 3]> = sorted_lab
        .into_iter()
        .map(|c| lab_to_rgb(c[0], c[1], c[2]))
        .collect();

    ThemeColors {
        primary_color,
        colors,
    }
}
