use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use image::open;
use randimg_core::color::extract_theme_colors;

fn bench_extract_theme_colors(c: &mut Criterion) {
    let img = open("tests/assets/test_image.jpg").expect("Failed to open test image");
    c.bench_function("extract_theme_colors_4800x2700", |b| {
        b.iter(|| {
            black_box(extract_theme_colors(black_box(&img)));
        })
    });
}

fn bench_kmeans_iterations(c: &mut Criterion) {
    use randimg_core::color::kmeans::kmeans;
    let data: Vec<[f32; 3]> = (0..100_000)
        .map(|i| {
            let l = (i % 100) as f32;
            let a = ((i * 7) % 256 - 128) as f32;
            let b = ((i * 13) % 256 - 128) as f32;
            [l, a, b]
        })
        .collect();

    let mut group = c.benchmark_group("kmeans");
    for batch_size in [None, Some(512), Some(1024), Some(2048), Some(4096)] {
        group.bench_with_input(
            BenchmarkId::new("batch_size", batch_size.unwrap_or(0)),
            &batch_size,
            |b, &bs| {
                b.iter(|| {
                    black_box(kmeans(black_box(&data), 10, 50, bs, false).0);
                })
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_extract_theme_colors, bench_kmeans_iterations);
criterion_main!(benches);
