use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, EnumIter, DeriveActiveEnum, Serialize, Deserialize, Hash,
)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "task_status")]
pub enum TaskStatus {
    #[sea_orm(string_value = "pending")]
    Pending,
    #[sea_orm(string_value = "queued")]
    Queued,
    #[sea_orm(string_value = "running")]
    Running,
    #[sea_orm(string_value = "done")]
    Done,
    #[sea_orm(string_value = "failed")]
    Failed,
    #[sea_orm(string_value = "killed")]
    Killed,
    #[sea_orm(string_value = "dead")]
    Dead,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Killed => "killed",
            Self::Dead => "dead",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Killed | Self::Dead)
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize, Hash,
)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "task_type")]
pub enum TaskType {
    #[sea_orm(string_value = "crawl")]
    Crawl,
    #[sea_orm(string_value = "download")]
    Download,
    #[sea_orm(string_value = "color_extract")]
    ColorExtract,
    #[sea_orm(string_value = "upload")]
    Upload,
    #[sea_orm(string_value = "accessibility_check")]
    AccessibilityCheck,
    #[sea_orm(string_value = "discover")]
    Discover,
    #[sea_orm(string_value = "refresh_pixiv_token")]
    RefreshPixivToken,
    #[sea_orm(string_value = "cleanup")]
    Cleanup,
}

impl TaskType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Crawl => "crawl",
            Self::Download => "download",
            Self::ColorExtract => "color_extract",
            Self::Upload => "upload",
            Self::AccessibilityCheck => "accessibility_check",
            Self::Discover => "discover",
            Self::RefreshPixivToken => "refresh_pixiv_token",
            Self::Cleanup => "cleanup",
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            "killed" => Ok(Self::Killed),
            "dead" => Ok(Self::Dead),
            _ => Err(format!("invalid task status: {s}")),
        }
    }
}

impl std::str::FromStr for TaskType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "crawl" => Ok(Self::Crawl),
            "download" => Ok(Self::Download),
            "color_extract" => Ok(Self::ColorExtract),
            "upload" => Ok(Self::Upload),
            "accessibility_check" => Ok(Self::AccessibilityCheck),
            "discover" => Ok(Self::Discover),
            "refresh_pixiv_token" => Ok(Self::RefreshPixivToken),
            "cleanup" => Ok(Self::Cleanup),
            _ => Err(format!("invalid task type: {s}")),
        }
    }
}
