/// Dead letter table entity
///
/// Stores tasks that have permanently failed (exceeded max retries).
/// Provides full failure metadata for auditing and potential recovery.
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "dead_letter")]
pub struct Model {
    /// Primary key (UUID)
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,

    /// Original task ID from the tasks table
    pub task_id: String,

    /// Task type: crawl, download, color_extract, upload, etc.
    pub task_type: String,

    /// Task parameters (JSON format)
    pub params: Option<String>,

    /// Final error message that caused the task to be moved to DLQ
    pub error_message: String,

    /// Retry count at the time of death
    pub retry_count: i32,

    /// History of all failures as JSON array: [{ "attempt": N, "error": "...", "timestamp": "..." }]
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub failure_history: Option<serde_json::Value>,

    /// When this entry was created (task died)
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
