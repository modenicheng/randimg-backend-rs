// Pixiv client integration using the pixiv-client crate
pub use pixiv_client::{PixivApi, PixivError, ApiResponse};
pub use pixiv_client::config::{Config, ClientConfig};
pub mod downloader {
    pub use pixiv_client::downloader::*;
}
pub use pixiv_client::models::illust::Illust;
pub use pixiv_client::models::common::{ImageUrls, MetaSinglePage, MetaPage, Tag};
pub use pixiv_client::models::user::UserPreview;

/// Create a PixivApi client, optionally configured with a proxy.
pub fn create_api(proxy: &str) -> PixivApi {
    if proxy.is_empty() {
        PixivApi::new()
    } else {
        let client_config = ClientConfig {
            proxy: Some(proxy.to_string()),
            ..Default::default()
        };
        PixivApi::with_config(Config::default(), client_config)
    }
}
