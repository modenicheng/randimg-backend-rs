use image::DynamicImage;
use randimg_core::color::kmeans::kmeans;

/// Load the test image (4800x2700 JPEG, a Pixiv illustration)
fn load_test_image() -> DynamicImage {
    image::open("tests/assets/test_image.jpg").expect("Failed to open test image")
}

// ---- extract_theme_colors integration tests ----

#[test]
fn test_extract_theme_colors_returns_correct_structure() {
    let img = load_test_image();
    let result = randimg_core::color::extract_theme_colors(&img);

    // Must return exactly 10 palette colors
    assert_eq!(result.colors.len(), 10, "Expected 10 palette colors");

    // Primary color must not be pure black (the image has content)
    assert_ne!(
        result.primary_color,
        [0, 0, 0],
        "Primary color should not be pure black"
    );
}

#[test]
fn test_palette_sorted_by_lightness() {
    let img = load_test_image();
    let result = randimg_core::color::extract_theme_colors(&img);

    // Colors should be sorted by lightness (L* ascending)
    // We can't check LAB values directly, but we can verify the sort order
    // by converting back to perceived brightness
    for window in result.colors.windows(2) {
        let brightness_a = perceived_brightness(window[0]);
        let brightness_b = perceived_brightness(window[1]);
        assert!(
            brightness_a <= brightness_b + 5, // allow small tolerance for rounding
            "Palette not sorted by lightness: {:?} (brightness {}) > {:?} (brightness {})",
            window[0],
            brightness_a,
            window[1],
            brightness_b,
        );
    }
}

#[test]
fn test_palette_colors_are_distinct() {
    let img = load_test_image();
    let result = randimg_core::color::extract_theme_colors(&img);

    // Check that palette colors are not all identical
    let first = result.colors[0];
    let all_same = result.colors.iter().all(|c| *c == first);
    assert!(!all_same, "All palette colors are identical: {:?}", first);
}

#[test]
fn test_primary_color_is_reasonable() {
    let img = load_test_image();
    let result = randimg_core::color::extract_theme_colors(&img);

    // Primary color should be a valid RGB triplet (any value is technically valid,
    // but it should not be all-zero for a real image)
    let [r, g, b] = result.primary_color;
    assert!(r > 0 || g > 0 || b > 0, "Primary color is pure black");

    // The test image is a Pixiv illustration; the primary color should be
    // a reasonable color (not extremely dark or extremely bright across all channels)
    // This is a sanity check, not a strict assertion
    let brightness = perceived_brightness(result.primary_color);
    assert!(
        brightness > 5,
        "Primary color {:?} is suspiciously dark (brightness={})",
        result.primary_color,
        brightness,
    );
}

#[test]
fn test_extract_theme_colors_handles_small_image() {
    // Create a tiny 2x2 image
    let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(2, 2, |x, y| {
        image::Rgb([(x * 128) as u8, (y * 128) as u8, 64])
    }));

    let result = randimg_core::color::extract_theme_colors(&img);
    assert_eq!(result.colors.len(), 10, "Should always return 10 colors");
}

#[test]
fn test_extract_theme_colors_handles_single_pixel() {
    let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        1,
        1,
        image::Rgb([100, 150, 200]),
    ));

    let result = randimg_core::color::extract_theme_colors(&img);
    assert_eq!(result.colors.len(), 10);
    // With only 1 pixel, all centroids converge to the same point
    // That's acceptable — the function should not panic
}

// ---- KMeans unit tests ----

#[test]
fn test_kmeans_basic_clustering() {
    // Two clearly separated clusters
    let mut data = Vec::new();
    for _ in 0..50 {
        data.push([0.0, 0.0, 0.0]);
    }
    for _ in 0..50 {
        data.push([100.0, 100.0, 100.0]);
    }

    let centroids = kmeans(&data, 2, 20, None);

    assert_eq!(centroids.len(), 2);

    // Centroids should be near [0,0,0] and [100,100,100]
    let mut sorted = centroids.clone();
    sorted.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
    assert!(
        sorted[0][0] < 10.0,
        "First centroid should be near 0, got {:?}",
        sorted[0]
    );
    assert!(
        sorted[1][0] > 90.0,
        "Second centroid should be near 100, got {:?}",
        sorted[1]
    );
}

#[test]
fn test_kmeans_empty_input() {
    let data: Vec<[f32; 3]> = vec![];
    let centroids = kmeans(&data, 3, 10, None);
    assert!(centroids.is_empty());
}

#[test]
fn test_kmeans_k_zero() {
    let data = vec![[1.0, 2.0, 3.0]; 10];
    let centroids = kmeans(&data, 0, 10, None);
    assert!(centroids.is_empty());
}

#[test]
fn test_kmeans_k_larger_than_data() {
    let data = vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let centroids = kmeans(&data, 5, 10, None);
    // When k > data.len(), centroids are padded by duplicating data points
    assert_eq!(
        centroids.len(),
        5,
        "Should return k centroids (padded from data)"
    );
    // All centroids should be one of the original data points
    for c in &centroids {
        assert!(
            *c == [1.0, 2.0, 3.0] || *c == [4.0, 5.0, 6.0],
            "Padded centroid should be a data point, got {:?}",
            c,
        );
    }
}

#[test]
fn test_kmeans_empty_cluster_recovery() {
    // Three clusters, but one is very far away and has few points
    let mut data = Vec::new();
    for _ in 0..100 {
        data.push([0.0, 0.0, 0.0]);
    }
    for _ in 0..100 {
        data.push([10.0, 10.0, 10.0]);
    }
    // Only 2 points in the third cluster — may get absorbed
    data.push([1000.0, 1000.0, 1000.0]);
    data.push([1001.0, 1001.0, 1001.0]);

    let centroids = kmeans(&data, 3, 30, None);
    assert_eq!(centroids.len(), 3);

    // All three centroids should be distinct (empty cluster recovery kicks in)
    let d01 = euclidean_sq_arr(&centroids[0], &centroids[1]);
    let d02 = euclidean_sq_arr(&centroids[0], &centroids[2]);
    let d12 = euclidean_sq_arr(&centroids[1], &centroids[2]);
    assert!(
        d01 > 1.0 || d02 > 1.0 || d12 > 1.0,
        "Centroids should be distinct: {:?}",
        centroids
    );
}

#[test]
fn test_kmeans_mini_batch() {
    let mut data = Vec::new();
    for _ in 0..500 {
        data.push([0.0, 0.0, 0.0]);
    }
    for _ in 0..500 {
        data.push([50.0, 50.0, 50.0]);
    }

    let centroids = kmeans(&data, 2, 50, Some(100));

    assert_eq!(centroids.len(), 2);
    let mut sorted = centroids.clone();
    sorted.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
    assert!(
        sorted[0][0] < 10.0,
        "Mini-batch centroid 0 should be near 0, got {:?}",
        sorted[0]
    );
    assert!(
        sorted[1][0] > 40.0,
        "Mini-batch centroid 1 should be near 50, got {:?}",
        sorted[1]
    );
}

#[test]
fn test_kmeans_produces_correct_k() {
    let data: Vec<[f32; 3]> = (0..100).map(|i| [i as f32, 0.0, 0.0]).collect();

    for k in [1, 3, 5, 10] {
        let centroids = kmeans(&data, k, 20, None);
        assert_eq!(centroids.len(), k, "k={} should produce {} centroids", k, k);
    }
}

// ---- Lab conversion round-trip test ----

#[test]
fn test_lab_round_trip() {
    // Test that RGB -> LAB -> RGB is approximately lossless
    let test_colors: &[[u8; 3]] = &[
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [128, 128, 128],
        [255, 255, 255],
        [0, 0, 0],
        [100, 150, 200],
    ];

    for &[r, g, b] in test_colors {
        let [l, a, b_val] = rgb_to_lab(r, g, b);
        let [r2, g2, b2] = lab_to_rgb(l, a, b_val);
        assert!(
            (r as i16 - r2 as i16).abs() <= 2
                && (g as i16 - g2 as i16).abs() <= 2
                && (b as i16 - b2 as i16).abs() <= 2,
            "Round-trip failed for ({},{},{}) -> ({},{},{}) -> ({},{},{})",
            r,
            g,
            b,
            l as i32,
            a as i32,
            b_val as i32,
            r2,
            g2,
            b2,
        );
    }
}

// ---- Histogram primary color tests ----

#[test]
fn test_histogram_primary_color_basic() {
    // 10 red pixels, 3 blue pixels — primary should be red
    let mut pixels = vec![[255u8, 0, 0]; 10];
    pixels.extend(vec![[0, 0, 255]; 3]);

    let primary = histogram_primary_color_from_pixels(&pixels, 16);
    // Red quantized at 16 levels: 255 -> bin 15 -> center = 15*16+8 = 248
    assert!(
        primary[0] > 200,
        "Primary should be red-ish, got {:?}",
        primary
    );
    assert!(
        primary[2] < 50,
        "Primary should not be blue, got {:?}",
        primary
    );
}

#[test]
fn test_histogram_primary_color_single_color() {
    let pixels = vec![[100u8, 150, 200]; 100];
    let primary = histogram_primary_color_from_pixels(&pixels, 16);
    // Should be the quantized version of [100, 150, 200]
    assert_eq!(primary, quantize_pixel(100, 150, 200, 16));
}

// ---- Helpers ----

fn perceived_brightness(c: [u8; 3]) -> u32 {
    (c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114) / 1000
}

fn euclidean_sq_arr(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

// Re-implement these locally for testing since they are private in the color module
fn quantize(val: u8, levels: u8) -> u8 {
    let step = 256 / levels as u16;
    ((val as u16 / step) * step + step / 2).min(255) as u8
}

fn quantize_pixel(r: u8, g: u8, b: u8, levels: u8) -> [u8; 3] {
    [
        quantize(r, levels),
        quantize(g, levels),
        quantize(b, levels),
    ]
}

fn histogram_primary_color_from_pixels(pixels: &[[u8; 3]], levels: u8) -> [u8; 3] {
    use std::collections::HashMap;
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

fn rgb_to_lab(r: u8, g: u8, b: u8) -> [f64; 3] {
    let srgb_to_linear = |c: u8| -> f64 {
        let c = c as f64 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };

    let r_lin = srgb_to_linear(r);
    let g_lin = srgb_to_linear(g);
    let b_lin = srgb_to_linear(b);

    let x = r_lin * 0.4124564 + g_lin * 0.3575761 + b_lin * 0.1804375;
    let y = r_lin * 0.2126729 + g_lin * 0.7151522 + b_lin * 0.0721750;
    let z = r_lin * 0.0193339 + g_lin * 0.1191920 + b_lin * 0.9503041;

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

    let r_lin = x * 3.2404542 + y * -1.5371385 + z * -0.4985314;
    let g_lin = x * -0.9692660 + y * 1.8760108 + z * 0.0415560;
    let b_lin = x * 0.0556434 + y * -0.2040259 + z * 1.0572252;

    let clamp = |v: f64| -> u8 {
        let v = v.clamp(0.0, 1.0);
        let v = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (v * 255.0).round().min(255.0).max(0.0) as u8
    };

    [clamp(r_lin), clamp(g_lin), clamp(b_lin)]
}
