/// 自定义任务表实体
///
/// 设计思路：
/// - 与 fang_tasks 表分离，避免直接依赖 fang 内部实现
/// - 存储业务相关的元数据（parent_id, root_id, crawler_id 等）
/// - 通过 fang_task_id 关联 fang 任务，便于状态同步
/// - 支持自定义状态流转，不受 fang 状态限制
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub use super::task_enum::{TaskStatus, TaskType};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tasks")]
pub struct Model {
    /// 主键，使用 UUID 或 ULID
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,

    /// Fang 任务 ID，用于关联 fang_tasks 表
    pub fang_task_id: Option<String>,

    /// 任务类型：crawl, download, color_extract, upload, accessibility_check, discover, refresh_pixiv_token
    pub task_type: TaskType,

    /// 任务状态：pending, queued, running, done, failed, killed
    pub status: TaskStatus,

    /// 父任务 ID，用于任务树结构
    pub parent_id: Option<String>,

    /// 根任务 ID，便于快速查询任务树
    pub root_id: Option<String>,

    /// 爬虫 ID（crawl 任务专用）
    pub crawler_id: Option<i32>,

    /// 图片 ID（download/upload 任务专用）
    pub image_id: Option<i32>,

    /// 任务参数（JSON 格式）
    pub params: Option<String>,

    /// 错误信息
    pub error_message: Option<String>,

    /// 重试次数
    pub retry_count: i32,

    /// 任务进度百分比 (0.0 ~ 100.0)
    pub progress: f64,

    /// 优先级（数值越小，优先级越高）
    pub priority: i32,

    /// 创建时间
    pub created_at: DateTimeWithTimeZone,

    /// 更新时间
    pub updated_at: DateTimeWithTimeZone,

    /// 完成时间
    pub completed_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
