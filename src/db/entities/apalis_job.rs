use sea_orm::entity::prelude::*;

/// SeaORM entity for the Apalis Jobs table.
///
/// The table is created and managed by Apalis migrations. This entity is
/// read-only from the application's perspective (list / get / delete).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[cfg_attr(feature = "sqlite", sea_orm(table_name = "Jobs"))]
#[cfg_attr(feature = "postgres", sea_orm(table_name = "jobs", schema_name = "apalis"))]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub job_type: String,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    #[cfg(feature = "sqlite")]
    pub run_at: i64,
    #[cfg(feature = "postgres")]
    pub run_at: DateTimeWithTimeZone,
    #[cfg(feature = "sqlite")]
    pub done_at: Option<i64>,
    #[cfg(feature = "postgres")]
    pub done_at: Option<DateTimeWithTimeZone>,
    #[cfg(feature = "sqlite")]
    pub last_result: Option<String>,
    #[cfg(feature = "postgres")]
    pub last_result: Option<JsonValue>,
    pub priority: i32,
    /// Serialized job payload (BLOB).
    pub job: Vec<u8>,
    /// When the worker locked this job for execution.
    #[cfg(feature = "sqlite")]
    pub lock_at: Option<i64>,
    #[cfg(feature = "postgres")]
    pub lock_at: Option<DateTimeWithTimeZone>,
    /// Worker ID that locked this job.
    pub lock_by: Option<String>,
    /// Arbitrary metadata JSON.
    #[cfg(feature = "sqlite")]
    pub metadata: Option<String>,
    #[cfg(feature = "postgres")]
    pub metadata: Option<JsonValue>,
    /// Idempotency key for deduplication.
    pub idempotency_key: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Apalis status constants (stored as TEXT in the Jobs table).
pub const STATUS_PENDING: &str = "Pending";
pub const STATUS_QUEUED: &str = "Queued";
pub const STATUS_RUNNING: &str = "Running";
pub const STATUS_DONE: &str = "Done";
pub const STATUS_FAILED: &str = "Failed";
pub const STATUS_KILLED: &str = "Killed";
