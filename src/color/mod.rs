pub mod kmeans;

use image::{DynamicImage, GenericImageView};
use rayon::prelude::*;
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

/// Extract primary color using histogram (mode of quantized colors), parallel.
fn histogram_primary_color(pixels: &[[u8; 3]], levels: u8) -> [u8; 3] {
    let hist: HashMap<[u8; 3], u32> = pixels
        .par_chunks(4096)
        .fold(
            || HashMap::new(),
            |mut acc, chunk| {
                for p in chunk {
                    let key = quantize_pixel(p[0], p[1], p[2], levels);
                    *acc.entry(key).or_insert(0) += 1;
                }
                acc
            },
        )
        .reduce(
            || HashMap::new(),
            |mut a, b| {
                for (k, v) in b {
                    *a.entry(k).or_insert(0) += v;
                }
                a
            },
        );

    hist.into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(color, _)| color)
        .unwrap_or([0, 0, 0])
}

// ---- RGB <-> CIELAB conversion ----

/// Precomputed sRGB-to-linear lookup table for u8 values [0, 255].
/// Avoids expensive powf(2.4) per pixel.
fn srgb_to_linear_lut() -> [f64; 256] {
    let mut lut = [0.0f64; 256];
    let mut i = 0;
    while i < 256 {
        let c = i as f64 / 255.0;
        lut[i] = if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        };
        i += 1;
    }
    lut
}

/// Precomputed linear-to-sRGB gamma compression LUT for fast LAB->RGB.
/// Indexed by linear value quantized to 0..4095.
fn linear_to_srgb_lut() -> [f64; 4096] {
    let mut lut = [0.0f64; 4096];
    let mut i = 0;
    while i < 4096 {
        let c = i as f64 / 4095.0;
        lut[i] = if c <= 0.0031308 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        i += 1;
    }
    lut
}

use std::sync::LazyLock;

static SRGB_TO_LINEAR: LazyLock<[f64; 256]> = LazyLock::new(srgb_to_linear_lut);
static LINEAR_TO_SRGB: LazyLock<[f64; 4096]> = LazyLock::new(linear_to_srgb_lut);

/// Convert sRGB [0,255] to CIELAB [L, a, b] using precomputed LUT.
fn rgb_to_lab(r: u8, g: u8, b: u8) -> [f64; 3] {
    let r_lin = SRGB_TO_LINEAR[r as usize];
    let g_lin = SRGB_TO_LINEAR[g as usize];
    let b_lin = SRGB_TO_LINEAR[b as usize];

    // linear RGB -> XYZ (D65 illuminant)
    let x = r_lin * 0.4124564 + g_lin * 0.3575761 + b_lin * 0.1804375;
    let y = r_lin * 0.2126729 + g_lin * 0.7151522 + b_lin * 0.0721750;
    let z = r_lin * 0.0193339 + g_lin * 0.1191920 + b_lin * 0.9503041;

    // XYZ -> LAB (D65 reference white)
    let f = |t: f64| -> f64 {
        const DELTA: f64 = 6.0 / 29.0;
        if t > DELTA.powi(3) {
            t.powf(1.0 / 3.0)
        } else {
            t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
        }
    };

    let fx = f(x / 0.95047);
    let fy = f(y);
    let fz = f(z / 1.08883);

    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Convert CIELAB to sRGB [0,255] using precomputed LUT.
fn lab_to_rgb(l: f64, a: f64, b: f64) -> [u8; 3] {
    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;

    let finv = |t: f64| -> f64 {
        const DELTA: f64 = 6.0 / 29.0;
        if t > DELTA { t * t * t } else { 3.0 * DELTA * DELTA * (t - 4.0 / 29.0) }
    };

    let x = finv(fx) * 0.95047;
    let y = finv(fy);
    let z = finv(fz) * 1.08883;

    let r_lin = x * 3.2404542 + y * -1.5371385 + z * -0.4985314;
    let g_lin = x * -0.9692660 + y * 1.8760108 + z * 0.0415560;
    let b_lin = x * 0.0556434 + y * -0.2040259 + z * 1.0572252;

    // Quantize linear [0,1] -> [0,4095] and use LUT for gamma compression
    let to_srgb = |v: f64| -> u8 {
        let v = v.clamp(0.0, 1.0);
        let idx = (v * 4095.0).round() as usize;
        let idx = idx.min(4095);
        (LINEAR_TO_SRGB[idx] * 255.0).round().min(255.0).max(0.0) as u8
    };

    [to_srgb(r_lin), to_srgb(g_lin), to_srgb(b_lin)]
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
    let small = img.resize_exact(new_w, new_h, image::imageops::FilterType::Nearest);
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

    // Convert to LAB for clustering (parallel)
    let lab_pixels: Vec<[f64; 3]> = pixels
        .par_iter()
        .map(|p| rgb_to_lab(p[0], p[1], p[2]))
        .collect();

    // KMeans clustering in LAB space
    let lab_centroids = kmeans::kmeans(&lab_pixels, 10, 50, Some(2048));

    // Sort by L* (lightness)
    let mut sorted_lab = lab_centroids;
    sorted_lab.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));

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
