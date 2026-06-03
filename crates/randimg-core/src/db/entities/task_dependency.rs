use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Parent-child relationship between tasks in the job queue.
///
/// This table tracks which task spawned which child tasks, enabling
/// visualization of task hierarchies for debugging and monitoring.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "task_dependencies")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// The parent task's UUID (references apalis.jobs.id).
    pub parent_job_id: String,
    /// The child task's UUID (references apalis.jobs.id).
    pub child_job_id: String,
    /// When this relationship was recorded.
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// Relation to the parent job.
    #[sea_orm(
        belongs_to = "super::apalis_job::Entity",
        from = "Column::ParentJobId",
        to = "super::apalis_job::Column::Id"
    )]
    ParentJob,
    /// Relation to the child job.
    #[sea_orm(
        belongs_to = "super::apalis_job::Entity",
        from = "Column::ChildJobId",
        to = "super::apalis_job::Column::Id"
    )]
    ChildJob,
}

impl Related<super::apalis_job::Entity> for Entity {
    fn to() -> RelationDef {
        // Default relation is to parent job
        Relation::ParentJob.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
