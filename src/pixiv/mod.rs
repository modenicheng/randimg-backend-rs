// Pixiv client integration using the pixiv-client crate
pub use pixiv_client::config::{ClientConfig, Config};
pub use pixiv_client::{ApiResponse, PixivApi, PixivError};
pub mod downloader {
    pub use pixiv_client::downloader::*;
}
pub use pixiv_client::models::common::{ImageUrls, MetaPage, MetaSinglePage, Tag};
pub use pixiv_client::models::illust::Illust;
pub use pixiv_client::models::user::UserPreview;

/// Create a PixivApi client, optionally configured with a proxy.
///
/// Sets `Accept-Language` header so Pixiv API responses include
/// `translated_name` on tags in the requested language.
pub async fn create_api(proxy: &str, accept_lang: &str) -> PixivApi {
    let api = if proxy.is_empty() {
        PixivApi::new()
    } else {
        let client_config = ClientConfig {
            proxy: Some(proxy.to_string()),
            ..Default::default()
        };
        PixivApi::with_config(Config::default(), client_config)
    };
    if !accept_lang.is_empty() {
        let _ = api.set_accept_lang(accept_lang).await;
    }
    api
}
