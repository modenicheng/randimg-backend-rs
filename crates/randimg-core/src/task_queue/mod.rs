pub mod fang_backend;
pub mod fingerprint;
pub mod handlers;
pub mod jobs;
pub mod retry;

/// Crawl type discriminator for `CrawlJob`.
///
/// Values match the `crawl_type` i32 stored in the database / job payload:
/// - 0 = Ranking
/// - 1 = User
/// - 2 = Bookmarks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrawlType {
    Ranking = 0,
    User = 1,
    Bookmarks = 2,
}

impl TryFrom<i32> for CrawlType {
    type Error = String;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Ranking),
            1 => Ok(Self::User),
            2 => Ok(Self::Bookmarks),
            _ => Err(format!("Invalid crawl_type: {}", value)),
        }
    }
}
