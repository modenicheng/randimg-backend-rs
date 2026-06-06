use image::GenericImageView;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let image_path = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("tests/assets/test_image.jpg");

    println!("Loading image: {}", image_path);
    let img = image::open(image_path).expect("Failed to open image");
    let (w, h) = img.dimensions();
    println!("  dimensions: {}x{}", w, h);

    // Warm-up run
    let _ = randimg_core::color::extract_theme_colors(&img);

    // Timed runs
    let runs = 5;
    let mut durations = Vec::new();

    for i in 0..runs {
        let start = Instant::now();
        let result = randimg_core::color::extract_theme_colors(&img);
        let elapsed = start.elapsed();
        durations.push(elapsed);

        if i == 0 {
            println!("\n=== Extraction Result (run 1) ===");
            println!(
                "  primary_color: RGB({},{},{})",
                result.primary_color[0], result.primary_color[1], result.primary_color[2]
            );
            println!(
                "  primary_lab:   L*{:.1} a*{:.1} b*{:.1}",
                result.primary_lab[0], result.primary_lab[1], result.primary_lab[2]
            );
            println!("  palette ({} colors, sorted by L*):", result.colors.len());
            for (j, (rgb, lab)) in result
                .colors
                .iter()
                .zip(result.colors_lab.iter())
                .enumerate()
            {
                let hex = format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
                let brightness =
                    (rgb[0] as u32 * 299 + rgb[1] as u32 * 587 + rgb[2] as u32 * 114) / 1000;
                println!(
                    "    [{:2}] #{} RGB({:3},{:3},{:3}) L*{:5.1} a*{:6.1} b*{:6.1} brightness={}",
                    j, hex, rgb[0], rgb[1], rgb[2], lab[0], lab[1], lab[2], brightness
                );
            }
        }
    }

    // Breakdown timing
    println!("\n=== Breakdown (single run) ===");
    let (w, h) = img.dimensions();

    let t0 = Instant::now();
    let max_dim = w.max(h) as f64;
    let scale = if max_dim > 1024.0 {
        1024.0 / max_dim
    } else {
        0.5
    };
    let new_w = ((w as f64 * scale) as u32).max(1);
    let new_h = ((h as f64 * scale) as u32).max(1);
    let small = img.resize_exact(new_w, new_h, image::imageops::FilterType::Nearest);
    let rgb = small.to_rgb8();
    let resize_elapsed = t0.elapsed();
    println!(
        "  resize ({}x{} -> {}x{}, scale={:.4}): {:?}",
        w, h, new_w, new_h, scale, resize_elapsed
    );

    let pixels: Vec<[u8; 3]> = rgb.pixels().map(|p| [p[0], p[1], p[2]]).collect();

    use rayon::prelude::*;
    let t1 = Instant::now();
    let lab_pixels: Vec<[f32; 3]> = pixels
        .par_iter()
        .map(|p| randimg_core::color::rgb_to_lab(p[0], p[1], p[2]))
        .collect();
    let lab_elapsed = t1.elapsed();
    println!(
        "  RGB->LAB conversion ({} pixels, par_iter): {:?}",
        lab_pixels.len(),
        lab_elapsed
    );

    let t2 = Instant::now();
    let (_centroids, _counts) =
        randimg_core::color::kmeans::kmeans(&lab_pixels, 10, 50, None, false);
    let kmeans_elapsed = t2.elapsed();
    println!("  KMeans (k=10, batch=2048, 50 iter): {:?}", kmeans_elapsed);

    let t3 = Instant::now();
    let _primary = histogram_primary_color_fn(&pixels, 16);
    let hist_elapsed = t3.elapsed();
    println!("  Histogram primary color: {:?}", hist_elapsed);

    // Stats
    durations.sort();
    let min = durations[0];
    let max = durations[runs - 1];
    let median = durations[runs / 2];
    let mean: std::time::Duration = durations.iter().sum::<std::time::Duration>() / runs as u32;

    println!("\n=== Performance ({} runs) ===", runs);
    println!("  min:    {:?}", min);
    println!("  max:    {:?}", max);
    println!("  median: {:?}", median);
    println!("  mean:   {:?}", mean);

    // Generate HTML visualization
    let result = randimg_core::color::extract_theme_colors(&img);
    let html = generate_html(&result, image_path, &durations);
    let out_path = "color_extract_result.html";
    std::fs::write(out_path, html).expect("Failed to write HTML");
    println!("\nVisualization saved to: {}", out_path);
}

fn histogram_primary_color_fn(pixels: &[[u8; 3]], levels: u8) -> [u8; 3] {
    use std::collections::HashMap;
    let step = 256 / levels as u16;
    let quantize = |val: u8| ((val as u16 / step) * step + step / 2).min(255) as u8;
    let mut hist: HashMap<[u8; 3], u32> = HashMap::new();
    for p in pixels {
        let key = [quantize(p[0]), quantize(p[1]), quantize(p[2])];
        *hist.entry(key).or_insert(0) += 1;
    }
    hist.into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(color, _)| color)
        .unwrap_or([0, 0, 0])
}

fn generate_html(
    result: &randimg_core::color::ThemeColors,
    image_path: &str,
    durations: &[std::time::Duration],
) -> String {
    let primary = result.primary_color;
    let primary_hex = format!("{:02X}{:02X}{:02X}", primary[0], primary[1], primary[2]);

    let palette_cells: String = result
        .colors
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let hex = format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
            let brightness =
                (c[0] as f64 * 0.299 + c[1] as f64 * 0.587 + c[2] as f64 * 0.114) as u32;
            let text_color = if brightness > 128 { "#000" } else { "#fff" };
            let lab = rgb_to_lab_display(c[0], c[1], c[2]);
            format!(
                r#"<div class="swatch" style="background:#{hex};color:{text_color}">
                    <div class="idx">#{}</div>
                    <div class="hex">#{hex}</div>
                    <div class="rgb">RGB({},{},{})</div>
                    <div class="lab">L*{:.1} a*{:.1} b*{:.1}</div>
                </div>"#,
                i, c[0], c[1], c[2], lab[0], lab[1], lab[2],
            )
        })
        .collect();

    let durations_str: String = durations
        .iter()
        .enumerate()
        .map(|(i, d)| {
            format!(
                "<tr><td>{}</td><td>{:.2} ms</td></tr>",
                i + 1,
                d.as_secs_f64() * 1000.0
            )
        })
        .collect();

    let min = durations.iter().min().unwrap();
    let max = durations.iter().max().unwrap();
    let mean_ms = durations.iter().sum::<std::time::Duration>().as_secs_f64() * 1000.0
        / durations.len() as f64;

    let image_name = std::path::Path::new(image_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| image_path.to_string());

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Color Extraction Result</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #1a1a2e; color: #eee; padding: 32px; }}
  h1 {{ font-size: 24px; margin-bottom: 8px; }}
  .meta {{ color: #888; margin-bottom: 24px; font-size: 14px; }}
  .section {{ margin-bottom: 32px; }}
  .section h2 {{ font-size: 18px; margin-bottom: 12px; color: #aaa; }}
  .primary {{ display: flex; align-items: center; gap: 16px; margin-bottom: 8px; }}
  .primary-swatch {{ width: 120px; height: 120px; border-radius: 12px; display: flex; align-items: center; justify-content: center; font-weight: bold; font-size: 14px; border: 2px solid rgba(255,255,255,0.1); }}
  .primary-info {{ font-size: 16px; line-height: 1.8; }}
  .palette {{ display: flex; gap: 8px; flex-wrap: wrap; }}
  .swatch {{ width: 140px; height: 140px; border-radius: 10px; padding: 10px; display: flex; flex-direction: column; justify-content: space-between; font-size: 11px; border: 2px solid rgba(255,255,255,0.05); }}
  .swatch .idx {{ font-weight: bold; font-size: 13px; }}
  .swatch .hex {{ font-family: monospace; font-size: 13px; }}
  .swatch .rgb {{ font-family: monospace; opacity: 0.8; }}
  .swatch .lab {{ font-family: monospace; opacity: 0.6; font-size: 10px; }}
  table {{ border-collapse: collapse; margin-top: 8px; }}
  th, td {{ padding: 6px 16px; text-align: left; border-bottom: 1px solid #333; }}
  th {{ color: #888; font-weight: normal; }}
  .stats {{ display: flex; gap: 32px; margin-top: 12px; }}
  .stat {{ background: #16213e; padding: 16px 24px; border-radius: 8px; }}
  .stat .val {{ font-size: 24px; font-weight: bold; color: #e94560; }}
  .stat .label {{ font-size: 12px; color: #888; margin-top: 4px; }}
</style>
</head>
<body>
<h1>Color Extraction Result</h1>
<p class="meta">Image: {image_name}</p>

<div class="section">
  <h2>Primary Color (Histogram Mode)</h2>
  <div class="primary">
    <div class="primary-swatch" style="background:#{primary_hex};color:{primary_text}">
      #{primary_hex}
    </div>
    <div class="primary-info">
      <div>RGB({pr},{pg},{pb})</div>
      <div>Hex: #{primary_hex}</div>
    </div>
  </div>
</div>

<div class="section">
  <h2>Color Palette (KMeans in CIELAB, sorted by L*)</h2>
  <div class="palette">
    {palette_cells}
  </div>
</div>

<div class="section">
  <h2>Performance</h2>
  <div class="stats">
    <div class="stat">
      <div class="val">{min_ms:.2} ms</div>
      <div class="label">Min</div>
    </div>
    <div class="stat">
      <div class="val">{mean_ms:.2} ms</div>
      <div class="label">Mean</div>
    </div>
    <div class="stat">
      <div class="val">{max_ms:.2} ms</div>
      <div class="label">Max</div>
    </div>
  </div>
  <table>
    <tr><th>Run</th><th>Duration</th></tr>
    {durations_str}
  </table>
</div>
</body>
</html>
"#,
        image_name = image_name,
        primary_hex = primary_hex,
        primary_text =
            if (primary[0] as u32 * 299 + primary[1] as u32 * 587 + primary[2] as u32 * 114) / 1000
                > 128
            {
                "#000"
            } else {
                "#fff"
            },
        pr = primary[0],
        pg = primary[1],
        pb = primary[2],
        palette_cells = palette_cells,
        min_ms = min.as_secs_f64() * 1000.0,
        max_ms = max.as_secs_f64() * 1000.0,
        mean_ms = mean_ms,
        durations_str = durations_str,
    )
}

fn rgb_to_lab_display(r: u8, g: u8, b: u8) -> [f64; 3] {
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
