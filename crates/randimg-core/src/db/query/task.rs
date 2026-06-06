/// 任务查询函数
///
/// 提供 tasks 表的 CRUD 操作，替代 apalis_job 查询模块。
/// 使用 SeaORM 查询构建器，PostgreSQL 查询。
use crate::db::entities::task::{self, Entity as Task};
use crate::db::entities::task_enum::{TaskStatus, TaskType};
use chrono::Utc;
use sea_orm::*;

/// 列出任务（分页，支持状态和类型过滤）
pub async fn list(
    db: &DatabaseConnection,
    task_type: Option<&TaskType>,
    status: Option<Vec<TaskStatus>>,
    limit: u64,
    offset: u64,
) -> Result<Vec<task::Model>, DbErr> {
    let mut query = Task::find().order_by_desc(task::Column::CreatedAt);
    if let Some(tt) = task_type {
        query = query.filter(task::Column::TaskType.eq(tt.clone()));
    }
    if let Some(ref statuses) = status {
        if !statuses.is_empty() {
            query = query.filter(task::Column::Status.is_in(statuses.iter().cloned()));
        }
    }
    query.limit(limit).offset(offset).all(db).await
}

/// 统计任务数量（支持状态和类型过滤）
pub async fn count(
    db: &DatabaseConnection,
    task_type: Option<&TaskType>,
    status: Option<Vec<TaskStatus>>,
) -> Result<u64, DbErr> {
    let mut query = Task::find();
    if let Some(tt) = task_type {
        query = query.filter(task::Column::TaskType.eq(tt.clone()));
    }
    if let Some(ref statuses) = status {
        if !statuses.is_empty() {
            query = query.filter(task::Column::Status.is_in(statuses.iter().cloned()));
        }
    }
    query.count(db).await
}

/// 按 ID 查找任务
pub async fn find_by_id(
    db: &DatabaseConnection,
    id: &str,
) -> Result<Option<task::Model>, DbErr> {
    Task::find_by_id(id.to_string()).one(db).await
}

/// 按 ID 删除任务
pub async fn delete_by_id(db: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
    let result = Task::delete_many()
        .filter(task::Column::Id.eq(id))
        .exec(db)
        .await?;

    Ok(result.rows_affected > 0)
}

/// 删除所有待处理任务（pending + queued）
pub async fn delete_pending(
    db: &DatabaseConnection,
    task_type: Option<&TaskType>,
) -> Result<u64, DbErr> {
    delete_by_statuses(
        db,
        &[TaskStatus::Pending, TaskStatus::Queued],
        task_type,
    )
    .await
}

/// 按状态删除任务，对根任务使用派生状态判断（避免删除有活跃子任务的根）。
pub async fn delete_by_statuses(
    db: &DatabaseConnection,
    statuses: &[TaskStatus],
    task_type: Option<&TaskType>,
) -> Result<u64, DbErr> {
    if statuses.is_empty() {
        return Ok(0);
    }

    let status_values: Vec<String> = statuses.iter().map(|s| format!("'{}'", s.as_str())).collect();
    let status_list = status_values.join(",");

    let mut type_filter = String::new();
    let mut bind_values: Vec<Value> = Vec::new();
    if let Some(tt) = task_type {
        bind_values.push(Value::from(tt.as_str()));
        type_filter = " AND t.task_type::text = $1".to_string();
    }

    let sql = format!(
        r#"
        WITH RECURSIVE
            descendants AS (
                SELECT t.id AS root_id, c.id AS descendant_id
                FROM tasks t
                JOIN tasks c ON c.parent_id = t.id
                WHERE t.parent_id IS NULL
                UNION ALL
                SELECT d.root_id, c.id
                FROM descendants d
                JOIN tasks c ON c.parent_id = d.descendant_id
            ),
            root_flags AS (
                SELECT
                    d.root_id,
                    BOOL_OR(t2.status::text IN ('pending','queued','running')) AS has_active,
                    BOOL_OR(t2.status::text IN ('failed','killed'))            AS has_failed,
                    BOOL_OR(t2.status::text = 'done')                          AS has_completed
                FROM descendants d
                JOIN tasks t2 ON t2.id = d.descendant_id
                GROUP BY d.root_id
            ),
            root_derived_status AS (
                SELECT
                    t.id AS root_id,
                    CASE
                        WHEN COALESCE(rf.has_active, false) THEN 'running'
                        WHEN COALESCE(rf.has_failed, false) AND COALESCE(rf.has_completed, false) THEN 'partial_success'
                        WHEN COALESCE(rf.has_failed, false) THEN 'failed'
                        WHEN COALESCE(rf.has_completed, false) THEN 'completed'
                        ELSE 'pending'
                    END AS derived_status
                FROM tasks t
                LEFT JOIN root_flags rf ON rf.root_id = t.id
                WHERE t.parent_id IS NULL
            ),
            deletable_roots AS (
                SELECT root_id FROM root_derived_status WHERE derived_status IN ({status_list})
            )
        DELETE FROM tasks
        WHERE id IN (SELECT root_id FROM deletable_roots)
           OR (parent_id IS NOT NULL AND status::text IN ({status_list}) {type_filter})
        "#
    );

    let stmt = Statement::from_sql_and_values(db.get_database_backend(), sql, bind_values);
    let result = db.execute(stmt).await?;

    Ok(result.rows_affected())
}

/// 按状态和 ID 列表批量删除任务
///
/// 只删除同时匹配状态列表和 ID 列表的任务。返回删除的任务数量。
pub async fn delete_by_statuses_and_ids(
    db: &DatabaseConnection,
    statuses: &[TaskStatus],
    ids: &[String],
) -> Result<u64, DbErr> {
    if statuses.is_empty() || ids.is_empty() {
        return Ok(0);
    }

    let result = Task::delete_many()
        .filter(task::Column::Status.is_in(statuses.iter().cloned()))
        .filter(task::Column::Id.is_in(ids.iter().map(|s| s.as_str())))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}

/// 更新任务状态
///
/// 当状态为终态（done/failed/killed）时自动设置 completed_at。
pub async fn update_status(
    db: &DatabaseConnection,
    id: &str,
    new_status: TaskStatus,
) -> Result<(), DbErr> {
    if let Some(t) = Task::find_by_id(id.to_string()).one(db).await? {
        let mut active: task::ActiveModel = t.into();
        active.status = Set(new_status.clone());
        active.updated_at = Set(Utc::now().into());

        // 终态自动设置完成时间
        if new_status.is_terminal() {
            active.completed_at = Set(Some(Utc::now().into()));
        }

        active.update(db).await?;
    }
    Ok(())
}

/// 更新任务错误信息
pub async fn update_error(
    db: &DatabaseConnection,
    id: &str,
    error_message: &str,
) -> Result<(), DbErr> {
    if let Some(t) = Task::find_by_id(id.to_string()).one(db).await? {
        let mut active: task::ActiveModel = t.into();
        active.error_message = Set(Some(error_message.to_string()));
        active.updated_at = Set(Utc::now().into());
        active.update(db).await?;
    }
    Ok(())
}

/// 递增重试次数
pub async fn increment_retry(db: &DatabaseConnection, id: &str) -> Result<(), DbErr> {
    if let Some(t) = Task::find_by_id(id.to_string()).one(db).await? {
        let mut active: task::ActiveModel = t.into();
        let current = active.retry_count.try_as_ref().copied().unwrap_or(0);
        active.retry_count = Set(current + 1);
        active.updated_at = Set(Utc::now().into());
        active.update(db).await?;
    }
    Ok(())
}

/// 创建新任务
pub async fn create(
    db: &impl ConnectionTrait,
    task_type: TaskType,
    parent_id: Option<&str>,
    root_id: Option<&str>,
    crawler_id: Option<i32>,
    image_id: Option<i32>,
    params: Option<serde_json::Value>,
) -> Result<task::Model, DbErr> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();

    let model = task::ActiveModel {
        id: Set(id),
        fang_task_id: Set(None),
        task_type: Set(task_type),
        status: Set(TaskStatus::Pending),
        parent_id: Set(parent_id.map(|s| s.to_string())),
        root_id: Set(root_id.map(|s| s.to_string())),
        crawler_id: Set(crawler_id),
        image_id: Set(image_id),
        params: Set(params),
        error_message: Set(None),
        retry_count: Set(0),
        priority: Set(0),
        progress: Set(0.0),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        completed_at: Set(None),
    };
    model.insert(db).await
}

/// 创建任务记录（使用指定的 ID）
pub async fn create_with_id(
    db: &impl ConnectionTrait,
    id: &str,
    task_type: TaskType,
    parent_id: Option<&str>,
    root_id: Option<&str>,
    crawler_id: Option<i32>,
    image_id: Option<i32>,
    params: Option<serde_json::Value>,
) -> Result<task::Model, DbErr> {
    let now = Utc::now();

    let model = task::ActiveModel {
        id: Set(id.to_string()),
        fang_task_id: Set(None),
        task_type: Set(task_type),
        status: Set(TaskStatus::Pending),
        parent_id: Set(parent_id.map(|s| s.to_string())),
        root_id: Set(root_id.map(|s| s.to_string())),
        crawler_id: Set(crawler_id),
        image_id: Set(image_id),
        params: Set(params),
        error_message: Set(None),
        retry_count: Set(0),
        priority: Set(0),
        progress: Set(0.0),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        completed_at: Set(None),
    };
    model.insert(db).await
}

/// 关联 Fang 任务 ID 并更新状态为 queued
pub async fn link_fang_task(
    db: &impl ConnectionTrait,
    task_id: &str,
    fang_task_id: &str,
) -> Result<(), DbErr> {
    if let Some(t) = Task::find_by_id(task_id.to_string()).one(db).await? {
        let mut active: task::ActiveModel = t.into();
        active.fang_task_id = Set(Some(fang_task_id.to_string()));
        active.status = Set(TaskStatus::Queued);
        active.updated_at = Set(Utc::now().into());
        active.update(db).await?;
    }
    Ok(())
}

/// Count tasks grouped by status category for health monitoring.
///
/// Returns `(running_count, queued_count, failed_count)` where:
/// - running: tasks with status "running"
/// - queued: tasks with status "pending" or "queued"
/// - failed: tasks with status "failed" or "killed"
pub async fn count_by_status(
    db: &DatabaseConnection,
) -> Result<(i64, i64, i64), DbErr> {
    let sql = r#"
        SELECT
            COUNT(CASE WHEN status = 'running' THEN 1 END) AS running_count,
            COUNT(CASE WHEN status IN ('pending', 'queued') THEN 1 END) AS queued_count,
            COUNT(CASE WHEN status IN ('failed', 'killed') THEN 1 END) AS failed_count
        FROM tasks
    "#;
    let stmt = Statement::from_sql_and_values(db.get_database_backend(), sql, []);
    let row = db.query_one(stmt).await?.ok_or(DbErr::RecordNotFound(
        "count_by_status returned no rows".to_string(),
    ))?;
    let running: i64 = row.try_get_by_index(0)?;
    let queued: i64 = row.try_get_by_index(1)?;
    let failed: i64 = row.try_get_by_index(2)?;
    Ok((running, queued, failed))
}

/// Task metrics snapshot for the `/metrics` endpoint.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskMetrics {
    pub total: i64,
    pub by_status: std::collections::HashMap<String, i64>,
    pub avg_duration_secs: Option<f64>,
    pub tasks_per_minute: f64,
}

/// Compute task queue metrics.
///
/// Returns total count, counts per status, average duration of completed
/// tasks (`completed_at - created_at`), and throughput (tasks completed in
/// the last 60 minutes divided by 60).
pub async fn get_task_metrics(db: &DatabaseConnection) -> Result<TaskMetrics, DbErr> {
    let sql = r#"
        SELECT
            COUNT(*)                                                       AS total,
            COUNT(CASE WHEN status = 'pending'   THEN 1 END)              AS cnt_pending,
            COUNT(CASE WHEN status = 'queued'    THEN 1 END)              AS cnt_queued,
            COUNT(CASE WHEN status = 'running'   THEN 1 END)              AS cnt_running,
            COUNT(CASE WHEN status = 'done'      THEN 1 END)              AS cnt_done,
            COUNT(CASE WHEN status = 'failed'    THEN 1 END)              AS cnt_failed,
            COUNT(CASE WHEN status = 'killed'    THEN 1 END)              AS cnt_killed,
            COUNT(CASE WHEN status = 'dead'      THEN 1 END)              AS cnt_dead,
            AVG(CASE
                WHEN completed_at IS NOT NULL
                THEN EXTRACT(EPOCH FROM (completed_at - created_at))
            END)                                                           AS avg_duration_secs,
            COUNT(CASE
                WHEN status = 'done'
                 AND completed_at >= NOW() - INTERVAL '1 hour'
                THEN 1
            END)                                                           AS completed_last_hour
        FROM tasks
    "#;
    let stmt = Statement::from_sql_and_values(db.get_database_backend(), sql, []);
    let row = db
        .query_one(stmt)
        .await?
        .ok_or(DbErr::RecordNotFound("get_task_metrics returned no rows".to_string()))?;

    let total: i64 = row.try_get_by_index(0)?;
    let cnt_pending: i64 = row.try_get_by_index(1)?;
    let cnt_queued: i64 = row.try_get_by_index(2)?;
    let cnt_running: i64 = row.try_get_by_index(3)?;
    let cnt_done: i64 = row.try_get_by_index(4)?;
    let cnt_failed: i64 = row.try_get_by_index(5)?;
    let cnt_killed: i64 = row.try_get_by_index(6)?;
    let cnt_dead: i64 = row.try_get_by_index(7)?;
    let avg_duration_secs: Option<f64> = row.try_get_by_index(8)?;
    let completed_last_hour: i64 = row.try_get_by_index(9)?;

    let mut by_status = std::collections::HashMap::with_capacity(7);
    by_status.insert("pending".to_string(), cnt_pending);
    by_status.insert("queued".to_string(), cnt_queued);
    by_status.insert("running".to_string(), cnt_running);
    by_status.insert("done".to_string(), cnt_done);
    by_status.insert("failed".to_string(), cnt_failed);
    by_status.insert("killed".to_string(), cnt_killed);
    by_status.insert("dead".to_string(), cnt_dead);

    let tasks_per_minute = completed_last_hour as f64 / 60.0;

    Ok(TaskMetrics {
        total,
        by_status,
        avg_duration_secs,
        tasks_per_minute,
    })
}

pub async fn delete_by_statuses_and_older_than(
    db: &DatabaseConnection,
    statuses: &[TaskStatus],
    older_than_hours: i64,
) -> Result<u64, DbErr> {
    if statuses.is_empty() || older_than_hours <= 0 {
        return Ok(0);
    }

    let cutoff = Utc::now() - chrono::Duration::hours(older_than_hours);

    let result = Task::delete_many()
        .filter(task::Column::Status.is_in(statuses.iter().cloned()))
        .filter(task::Column::CompletedAt.is_not_null())
        .filter(task::Column::CompletedAt.lt(cutoff))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}
