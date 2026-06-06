pub mod helpers;

pub mod accessibility_check;
pub mod cleanup;
pub mod color_extract;
pub mod crawl;
pub mod discover;
pub mod download;
pub mod refresh_pixiv_token;
pub mod upload;

// Re-export all handler functions so that jobs.rs's `handlers::$handler()` path works.
pub use accessibility_check::handle_accessibility_check;
pub use cleanup::handle_cleanup;
pub use color_extract::handle_color_extract;
pub use crawl::handle_crawl;
pub use discover::handle_discover;
pub use download::handle_download;
pub use refresh_pixiv_token::handle_refresh_pixiv_token;
pub use upload::handle_upload;
