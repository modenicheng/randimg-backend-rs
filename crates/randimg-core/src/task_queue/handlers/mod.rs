pub mod helpers;

pub mod crawl;
pub mod download;
pub mod color_extract;
pub mod upload;
pub mod accessibility_check;
pub mod discover;
pub mod refresh_pixiv_token;
pub mod cleanup;

// Re-export all handler functions so that jobs.rs's `handlers::$handler()` path works.
pub use crawl::handle_crawl;
pub use download::handle_download;
pub use color_extract::handle_color_extract;
pub use upload::handle_upload;
pub use accessibility_check::handle_accessibility_check;
pub use discover::handle_discover;
pub use refresh_pixiv_token::handle_refresh_pixiv_token;
pub use cleanup::handle_cleanup;
