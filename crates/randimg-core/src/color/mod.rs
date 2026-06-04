pub mod kmeans;

use image::{DynamicImage, GenericImageView};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Dedicated rayon thread pool for color extraction.
/// Initialized once with the configured number of threads.
static COLOR_POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();

/// Initialize (or return existing) dedicated color extraction thread pool.
///
/// `threads`: number of threads in the pool. Panics if called with different
/// values after initialization (OnceLock semantics).
pub fn init_color_pool(threads: usize) -> &'static rayon::ThreadPool {
    COLOR_POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|idx| format!("color-worker-{}", idx))
            .build()
            .expect("Failed to create color extraction rayon pool")
    })
}

/// Run a closure on the dedicated color extraction thread pool.
///
/// This ensures color extraction work runs on isolated threads that don't
/// compete with tokio's async worker threads or the global rayon pool.
pub fn run_on_color_pool<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    let pool = init_color_pool(
        std::env::var("COLOR_WORKER_RAYON_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4)
            }),
    );
    pool.install(f)
}

#[derive(Serialize)]
pub struct ColorEntry {
    pub rgb: [u8; 3],
    pub lab: [f32; 3],
}

#[derive(Serialize)]
pub struct ThemeColors {
    pub primary_color: [u8; 3],
    pub primary_lab: [f32; 3],
    pub colors: Vec<[u8; 3]>,
    pub colors_lab: Vec<[f32; 3]>,
}

/// Quantize a channel value into bins (e.g., 16 levels)
fn quantize(val: u8, levels: u8) -> u8 {
    let step = 256 / levels as u16;
    ((val as u16 / step) * step + step / 2).min(255) as u8
}

/// Quantize RGB into a bin key
fn quantize_pixel(r: u8, g: u8, b: u8, levels: u8) -> [u8; 3] {
    [
        quantize(r, levels),
        quantize(g, levels),
        quantize(b, levels),
    ]
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
///
/// LUTs remain f64 for conversion precision; result converted to f32 at the boundary.
pub fn rgb_to_lab(r: u8, g: u8, b: u8) -> [f32; 3] {
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

    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let b_val = 200.0 * (fy - fz);

    [l as f32, a as f32, b_val as f32]
}

/// Convert CIELAB to sRGB [0,255] using precomputed LUT.
///
/// Accepts f32 inputs (from kmeans/extract_theme_colors), converts to f64 internally
/// for LUT indexing precision.
fn lab_to_rgb(l: f32, a: f32, b: f32) -> [u8; 3] {
    let l = l as f64;
    let a = a as f64;
    let b = b as f64;
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
///
/// All CPU-heavy work (image resize, LAB conversion, KMeans) runs on a
/// dedicated rayon thread pool so it doesn't compete with tokio's async
/// executor or the global rayon pool.
pub fn extract_theme_colors(img: &DynamicImage) -> ThemeColors {
    extract_theme_colors_with_config(img, 12, 50, 2048, 0.5)
}

    pub fn extract_theme_colors_with_config(
        img: &DynamicImage,
        k: usize,
        max_iter: usize,
        _batch_size: usize,
        image_scale: f64,
    ) -> ThemeColors {
        run_on_color_pool(|| {
            let (w, h) = img.dimensions();
            let max_dim = w.max(h) as f64;
            let scale = if max_dim > 1024.0 { 1024.0 / max_dim } else { image_scale };
            let new_w = ((w as f64 * scale) as u32).max(1);
            let new_h = ((h as f64 * scale) as u32).max(1);
        let small = img.resize_exact(new_w, new_h, image::imageops::FilterType::Nearest);
        let rgb = small.to_rgb8();

        let pixels: Vec<[u8; 3]> = rgb.pixels().map(|p| [p[0], p[1], p[2]]).collect();

        if pixels.is_empty() {
            return ThemeColors {
                primary_color: [0, 0, 0],
                primary_lab: [0.0f32; 3],
                colors: vec![[0, 0, 0]; k],
                colors_lab: vec![[0.0f32; 3]; k],
            };
        }

        let primary_color = histogram_primary_color(&pixels, 16);
        let primary_lab = rgb_to_lab(primary_color[0], primary_color[1], primary_color[2]);

        let lab_pixels: Vec<[f32; 3]> = pixels
            .par_iter()
            .map(|p| rgb_to_lab(p[0], p[1], p[2]))
            .collect();

        // NOTE: 使用全量 KMeans 而非 mini-batch。Mini-batch 的 stride 采样会
        // 遗漏稀疏像素（如点缀色），导致异常颜色进入调色板（如图片 680/773 的
        // 幻影绿色）。全量模式 ~1.8s vs mini-batch ~6ms，但准确性优先。
        let (lab_centroids, counts) =
            kmeans::kmeans(&lab_pixels, k, max_iter, None, false);

        // 按像素数排序，取前 10 个最大的簇
        let mut cluster_pairs: Vec<_> = lab_centroids.into_iter().zip(counts.into_iter()).collect();
        cluster_pairs.sort_by(|a, b| b.1.cmp(&a.1));
        let top_k = 10.min(cluster_pairs.len());
        let final_centroids: Vec<[f32; 3]> = cluster_pairs[..top_k].iter().map(|(c, _)| *c).collect();

        let mut sorted_lab = final_centroids;
        sorted_lab.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));

        let colors_lab: Vec<[f32; 3]> = sorted_lab.clone();
        let colors: Vec<[u8; 3]> = sorted_lab
            .into_iter()
            .map(|c| lab_to_rgb(c[0], c[1], c[2]))
            .collect();

        ThemeColors {
            primary_color,
            primary_lab,
            colors,
            colors_lab,
        }
    })
}
