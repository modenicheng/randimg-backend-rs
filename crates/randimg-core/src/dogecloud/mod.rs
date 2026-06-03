//! DogeCloud OSS 凭证管理层。
//!
//! 架构:
//! - `api` — 调用多吉云 Console API 获取临时凭证（HMAC-SHA1 签名）
//! - `oss` — 封装 AWS S3 SDK，带自动凭证刷新
//! - `types` — 凭证、配置、缓存类型
//!
//! 使用方式:
//! ```ignore
//! let oss = DogeCloudOss::new(access_key, secret_key);
//! oss.upload("path/to/file.jpg", bytes).await?;
//! let data = oss.download("path/to/file.jpg").await?;
//! ```

pub mod api;
pub mod oss;
mod types;

pub use oss::DogeCloudOss;
