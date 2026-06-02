//! DogeCloud OSS 上传示例
//!
//! 用法:
//!   cargo run --example oss_upload_demo [本地文件路径] [OSS 对象键]
//!
//! 示例:
//!   cargo run --example oss_upload_demo tests/assets/test_image.jpg test/demo.jpg
//!   cargo run --example oss_upload_demo                          # 使用默认值

use std::time::Instant;

#[tokio::main]
async fn main() {
    // 加载 .env
    dotenvy::dotenv().ok();

    // 初始化简单日志
    tracing_subscriber::fmt()
        .with_env_filter("oss_upload_demo=debug,randimg_backend_rs=debug")
        .init();

    let args: Vec<String> = std::env::args().collect();
    let local_path = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("tests/assets/test_image.jpg");
    let oss_key = args
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or("test/upload_demo.jpg");

    println!("=== DogeCloud OSS 上传演示 ===");
    println!("  本地文件: {local_path}");
    println!("  OSS Key:  {oss_key}");
    println!();

    // 从环境变量构建配置
    let config = randimg_backend_rs::config::AppConfig::from_env();
    let oss = randimg_backend_rs::dogecloud::DogeCloudOss::new(&config, reqwest::Client::new());

    // ── 上传 ──────────────────────────────────────────
    println!("[1/3] 读取本地文件...");
    let bytes = match tokio::fs::read(local_path).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  ✗ 读取失败: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "  ✓ 文件大小: {} bytes ({:.2} KB)",
        bytes.len(),
        bytes.len() as f64 / 1024.0
    );

    println!("[2/3] 上传到 OSS...");
    let t0 = Instant::now();
    match oss.upload(oss_key, bytes.clone()).await {
        Ok(()) => {
            println!("  ✓ 上传成功! 耗时 {:?}", t0.elapsed());
            let cdn_url = format!(
                "{}{}",
                config.cdn_base_url.trim_end_matches('/'),
                if oss_key.starts_with('/') {
                    oss_key.to_string()
                } else {
                    format!("/{oss_key}")
                }
            );
            println!("  CDN 地址: {cdn_url}");
        }
        Err(e) => {
            eprintln!("  ✗ 上传失败: {e:#}");
            std::process::exit(1);
        }
    }

    // ── 下载验证 ────────────────────────────────────────
    println!("[3/3] 下载验证...");
    let t1 = Instant::now();
    match oss.download(oss_key).await {
        Ok(downloaded) => {
            println!("  ✓ 下载成功! 耗时 {:?}", t1.elapsed());
            println!("  下载大小: {} bytes", downloaded.len());
            if downloaded == bytes {
                println!("  ✓ 内容一致性校验通过!");
            } else {
                println!(
                    "  ✗ 内容不一致! 上传 {} bytes vs 下载 {} bytes",
                    bytes.len(),
                    downloaded.len()
                );
            }
        }
        Err(e) => {
            eprintln!("  ✗ 下载失败: {e:#}");
            // 下载失败不退出，上传已成功
        }
    }

    println!("\n=== 完成 ===");
}
