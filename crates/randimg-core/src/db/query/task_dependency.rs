use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set};

use crate::db::entities::task_dependency::{self, Entity as TaskDependency, Model as TaskDepModel};

/// Record a parent-child relationship between two jobs.
///
/// This is called at the start of child job execution when `parent_job_id` is
/// present in the job payload. The relationship is recorded reactively because
/// `TaskSink::push()` does not return the child job ID.
pub async fn record(
    db: &DatabaseConnection,
    parent_job_id: &str,
    child_job_id: &str,
) -> Result<(), sea_orm::DbErr> {
    // Avoid duplicates
    let existing = TaskDependency::find()
        .filter(task_dependency::Column::ParentJobId.eq(parent_job_id))
        .filter(task_dependency::Column::ChildJobId.eq(child_job_id))
        .one(db)
        .await?;

    if existing.is_some() {
        return Ok(());
    }

    let now = chrono::Utc::now().naive_utc();
    let dep = task_dependency::ActiveModel {
        id: sea_orm::NotSet,
        parent_job_id: Set(parent_job_id.to_string()),
        child_job_id: Set(child_job_id.to_string()),
        created_at: Set(now),
    };
    dep.insert(db).await?;
    Ok(())
}

/// Get all direct child jobs of a given parent job.
pub async fn get_children(
    db: &DatabaseConnection,
    parent_job_id: &str,
) -> Result<Vec<TaskDepModel>, sea_orm::DbErr> {
    TaskDependency::find()
        .filter(task_dependency::Column::ParentJobId.eq(parent_job_id))
        .order_by_asc(task_dependency::Column::CreatedAt)
        .all(db)
        .await
}

/// Get the parent of a given job (if any).
pub async fn get_parent(
    db: &DatabaseConnection,
    child_job_id: &str,
) -> Result<Option<TaskDepModel>, sea_orm::DbErr> {
    TaskDependency::find()
        .filter(task_dependency::Column::ChildJobId.eq(child_job_id))
        .one(db)
        .await
}

/// A node in the task tree, containing the dependency record and its children.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskTreeNode {
    pub dep: TaskDepModel,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TaskTreeNode>,
}

/// Recursively build children for a given parent job ID.
async fn build_tree_recursive(
    db: &DatabaseConnection,
    parent_job_id: &str,
) -> Result<Vec<TaskTreeNode>, sea_orm::DbErr> {
    let children = get_children(db, parent_job_id).await?;
    let mut nodes = Vec::with_capacity(children.len());

    for child in children {
        let grandchildren = Box::pin(build_tree_recursive(db, &child.child_job_id)).await?;
        nodes.push(TaskTreeNode {
            dep: child,
            children: grandchildren,
        });
    }

    Ok(nodes)
}

/// Get the full task tree rooted at `root_job_id`.
///
/// Uses recursive async (via `Box::pin`) to handle arbitrarily deep trees.
pub async fn get_task_tree(
    db: &DatabaseConnection,
    root_job_id: &str,
) -> Result<Vec<TaskTreeNode>, sea_orm::DbErr> {
    build_tree_recursive(db, root_job_id).await
}
