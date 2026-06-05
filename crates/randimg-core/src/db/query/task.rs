/// 任务查询函数
///
/// 提供 tasks 表的 CRUD 操作，替代 apalis_job 查询模块。
/// 使用 SeaORM 查询构建器，PostgreSQL 查询。
use crate::db::entities::task::{
    self, Entity as Task, STATUS_DEAD, STATUS_DONE, STATUS_FAILED, STATUS_KILLED, STATUS_PENDING,
    STATUS_QUEUED, STATUS_RUNNING,
};
use chrono::Utc;
use sea_orm::*;

/// 列出任务（分页，支持状态和类型过滤）
pub async fn list(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    status: Option<Vec<&str>>,
    limit: u64,
    offset: u64,
) -> Result<Vec<task::Model>, DbErr> {
    let mut query = Task::find().order_by_desc(task::Column::CreatedAt);
    if let Some(tt) = task_type {
        query = query.filter(task::Column::TaskType.eq(tt));
    }
    if let Some(ref statuses) = status {
        if !statuses.is_empty() {
            query = query.filter(task::Column::Status.is_in(statuses.iter().copied()));
        }
    }
    query.limit(limit).offset(offset).all(db).await
}

/// 统计任务数量（支持状态和类型过滤）
pub async fn count(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    status: Option<Vec<&str>>,
) -> Result<u64, DbErr> {
    let mut query = Task::find();
    if let Some(tt) = task_type {
        query = query.filter(task::Column::TaskType.eq(tt));
    }
    if let Some(ref statuses) = status {
        if !statuses.is_empty() {
            query = query.filter(task::Column::Status.is_in(statuses.iter().copied()));
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
    task_type: Option<&str>,
) -> Result<u64, DbErr> {
    delete_by_statuses(
        db,
        &[task::STATUS_PENDING, task::STATUS_QUEUED],
        task_type,
    )
    .await
}

/// 按状态批量删除任务，返回删除的任务数量。
pub async fn delete_by_statuses(
    db: &DatabaseConnection,
    statuses: &[&str],
    task_type: Option<&str>,
) -> Result<u64, DbErr> {
    if statuses.is_empty() {
        return Ok(0);
    }

    let mut q = Task::delete_many()
        .filter(task::Column::Status.is_in(statuses.iter().copied()));
    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }

    let result = q.exec(db).await?;
    Ok(result.rows_affected)
}

/// 按状态和 ID 列表批量删除任务
///
/// 只删除同时匹配状态列表和 ID 列表的任务。返回删除的任务数量。
pub async fn delete_by_statuses_and_ids(
    db: &DatabaseConnection,
    statuses: &[&str],
    ids: &[String],
) -> Result<u64, DbErr> {
    if statuses.is_empty() || ids.is_empty() {
        return Ok(0);
    }

    let result = Task::delete_many()
        .filter(task::Column::Status.is_in(statuses.iter().copied()))
        .filter(task::Column::Id.is_in(ids.iter().map(|s| s.as_str())))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}

/// 按任务类型查找爬虫 ID 列表
///
/// 用于查询指定 crawl_type 的 crawl 任务对应的 crawler_id。
pub async fn find_crawl_ids_by_type(
    db: &DatabaseConnection,
    crawl_type: i32,
) -> Result<Vec<i32>, DbErr> {
    let sql = "SELECT crawler_id FROM tasks WHERE task_type = 'crawl' AND (params::json->>'crawl_type')::int = $1 AND crawler_id IS NOT NULL";
    let stmt = Statement::from_sql_and_values(
        db.get_database_backend(),
        sql,
        [crawl_type.into()],
    );
    let rows = db.query_all(stmt).await?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in &rows {
        ids.push(row.try_get_by_index::<i32>(0)?);
    }
    Ok(ids)
}

/// 更新任务状态
///
/// 当状态为终态（done/failed/killed）时自动设置 completed_at。
pub async fn update_status(
    db: &DatabaseConnection,
    id: &str,
    new_status: &str,
) -> Result<(), DbErr> {
    if let Some(t) = Task::find_by_id(id.to_string()).one(db).await? {
        let mut active: task::ActiveModel = t.into();
        active.status = Set(new_status.to_string());
        active.updated_at = Set(Utc::now().into());

        // 终态自动设置完成时间
        if new_status == STATUS_DONE
            || new_status == STATUS_FAILED
            || new_status == STATUS_KILLED
            || new_status == STATUS_DEAD
        {
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
    task_type: &str,
    parent_id: Option<&str>,
    root_id: Option<&str>,
    crawler_id: Option<i32>,
    image_id: Option<i32>,
    params: Option<&str>,
) -> Result<task::Model, DbErr> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();

    let model = task::ActiveModel {
        id: Set(id),
        fang_task_id: Set(None),
        task_type: Set(task_type.to_string()),
        status: Set(task::STATUS_PENDING.to_string()),
        parent_id: Set(parent_id.map(|s| s.to_string())),
        root_id: Set(root_id.map(|s| s.to_string())),
        crawler_id: Set(crawler_id),
        image_id: Set(image_id),
        params: Set(params.map(|s| s.to_string())),
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
    task_type: &str,
    parent_id: Option<&str>,
    root_id: Option<&str>,
    crawler_id: Option<i32>,
    image_id: Option<i32>,
    params: Option<&str>,
) -> Result<task::Model, DbErr> {
    let now = Utc::now();

    let model = task::ActiveModel {
        id: Set(id.to_string()),
        fang_task_id: Set(None),
        task_type: Set(task_type.to_string()),
        status: Set(task::STATUS_PENDING.to_string()),
        parent_id: Set(parent_id.map(|s| s.to_string())),
        root_id: Set(root_id.map(|s| s.to_string())),
        crawler_id: Set(crawler_id),
        image_id: Set(image_id),
        params: Set(params.map(|s| s.to_string())),
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
        active.status = Set(task::STATUS_QUEUED.to_string());
        active.updated_at = Set(Utc::now().into());
        active.update(db).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 任务树查询（基于 root_id 的扁平化设计）
// ---------------------------------------------------------------------------

/// 根任务及其派生状态
///
/// 用于 API 返回，包含任务基本字段和从子任务计算出的派生状态。
#[derive(Debug, Clone, serde::Serialize)]
pub struct RootTaskWithStatus {
    pub id: String,
    pub task_type: String,
    pub status: String,
    pub root_id: Option<String>,
    pub crawler_id: Option<i32>,
    pub image_id: Option<i32>,
    pub params: Option<String>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    /// 从子任务计算出的派生状态
    pub derived_status: String,
    /// 子任务总数
    pub children_count: u64,
}

/// 获取任务树（扁平列表）
///
/// 通过 `root_id` 一次性查询整个任务树，避免递归 CTE。
/// 根任务自身的 `root_id` 可以为 `None`（自身即为根），此时使用其 `id` 查询。
/// 返回结果按 `created_at` 升序排列（根任务在前）。
pub async fn get_task_tree(
    db: &DatabaseConnection,
    root_id: &str,
) -> Result<Vec<task::Model>, DbErr> {
    Task::find()
        .filter(task::Column::RootId.eq(root_id))
        .order_by_asc(task::Column::CreatedAt)
        .all(db)
        .await
}

/// 计算任务树的派生状态
///
/// 根据子任务状态聚合得出根任务的派生状态：
/// - 有任何 running/pending/queued 子任务 → "running"
/// - 全部子任务完成且无失败 → "completed"
/// - 有失败但也有完成 → "partial_success"
/// - 全部失败或被杀 → "failed"
/// - 无子任务 → 使用根任务自身状态
pub async fn compute_derived_status(
    db: &DatabaseConnection,
    root_id: &str,
) -> Result<String, DbErr> {
    let children: Vec<task::Model> = Task::find()
        .filter(task::Column::RootId.eq(root_id))
        .filter(task::Column::Id.ne(root_id))
        .all(db)
        .await?;

    if children.is_empty() {
        return if let Some(root) = Task::find_by_id(root_id.to_string()).one(db).await? {
            Ok(root.status)
        } else {
            Ok(STATUS_PENDING.to_string())
        };
    }

    let total = children.len() as u64;
    let mut active_count: u64 = 0;
    let mut completed_count: u64 = 0;
    let mut failed_count: u64 = 0;

    for child in &children {
        match child.status.as_str() {
            STATUS_PENDING | STATUS_QUEUED | STATUS_RUNNING => active_count += 1,
            STATUS_DONE => completed_count += 1,
            STATUS_FAILED | STATUS_KILLED => failed_count += 1,
            _ => {}
        }
    }

    let derived = if active_count > 0 {
        "running"
    } else if failed_count > 0 && completed_count > 0 {
        "partial_success"
    } else if failed_count == total {
        "failed"
    } else if completed_count == total {
        "completed"
    } else {
        "pending"
    };

    Ok(derived.to_string())
}

/// 列出根任务（带派生状态，分页）
///
/// 根任务定义：`parent_id IS NULL`。
/// 每个根任务附带从子任务计算出的派生状态和子任务总数。
///
/// 使用单条 SQL 查询（LEFT JOIN + GROUP BY）代替原来的 N+1 查询模式：
/// - 原实现：1 次查询根任务 + 每个根任务 2 次查询（派生状态 + 子任务计数）= 2N+1 次
/// - 新实现：1 次查询，通过 LEFT JOIN 聚合子任务状态计数，在 Rust 中计算派生状态
pub async fn list_roots(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    limit: u64,
    offset: u64,
) -> Result<Vec<RootTaskWithStatus>, DbErr> {
    let mut bind_values: Vec<Value> = Vec::new();
    bind_values.push(Value::from(limit as i64));
    bind_values.push(Value::from(offset as i64));

    let mut type_filter = String::new();
    if let Some(tt) = task_type {
        bind_values.push(Value::from(tt));
        type_filter = " AND r.task_type = $3".to_string();
    }

    let sql = format!(
        r#"
        SELECT
            r.id, r.task_type, r.status, r.root_id, r.crawler_id, r.image_id,
            r.params, r.error_message, r.retry_count,
            r.created_at, r.updated_at, r.completed_at,
            COUNT(c.id)                                                      AS children_count,
            COUNT(CASE WHEN c.status IN ('pending','queued','running') THEN 1 END) AS active_count,
            COUNT(CASE WHEN c.status = 'done'              THEN 1 END) AS done_count,
            COUNT(CASE WHEN c.status IN ('failed','killed') THEN 1 END) AS failed_count,
            COUNT(CASE WHEN c.status = 'killed'             THEN 1 END) AS killed_count
        FROM (
            SELECT * FROM tasks
            WHERE parent_id IS NULL{type_filter}
            ORDER BY created_at DESC
            LIMIT $1 OFFSET $2
        ) r
        LEFT JOIN tasks c ON c.root_id = r.id AND c.id <> r.id
        GROUP BY r.id, r.task_type, r.status, r.root_id, r.crawler_id, r.image_id,
                 r.params, r.error_message, r.retry_count,
                 r.created_at, r.updated_at, r.completed_at
        ORDER BY r.created_at DESC
        "#
    );

    let stmt = Statement::from_sql_and_values(db.get_database_backend(), sql, bind_values);
    let rows = db.query_all(stmt).await?;

    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: String = row.try_get_by_index(0)?;
        let task_type_val: String = row.try_get_by_index(1)?;
        let status: String = row.try_get_by_index(2)?;
        let root_id: Option<String> = row.try_get_by_index(3)?;
        let crawler_id: Option<i32> = row.try_get_by_index(4)?;
        let image_id: Option<i32> = row.try_get_by_index(5)?;
        let params: Option<String> = row.try_get_by_index(6)?;
        let error_message: Option<String> = row.try_get_by_index(7)?;
        let retry_count: i32 = row.try_get_by_index(8)?;
        let created_at: chrono::DateTime<chrono::Utc> = row.try_get_by_index(9)?;
        let updated_at: chrono::DateTime<chrono::Utc> = row.try_get_by_index(10)?;
        let completed_at: Option<chrono::DateTime<chrono::Utc>> = row.try_get_by_index(11)?;
        let children_count: i64 = row.try_get_by_index(12)?;
        let active_count: i64 = row.try_get_by_index(13)?;
        let done_count: i64 = row.try_get_by_index(14)?;
        let failed_count: i64 = row.try_get_by_index(15)?;

        // Compute derived status from aggregated child counts.
        // Mirrors the logic of `compute_derived_status()` but in-memory:
        // - Any active (pending/queued/running) children → "running"
        // - All children done, none failed → "completed"
        // - Some done + some failed → "partial_success"
        // - All failed → "failed"
        // - No children → use root's own status
        let derived_status = if children_count == 0 {
            status.clone()
        } else if active_count > 0 {
            "running".to_string()
        } else if failed_count > 0 && done_count > 0 {
            "partial_success".to_string()
        } else if failed_count == children_count {
            "failed".to_string()
        } else if done_count == children_count {
            "completed".to_string()
        } else {
            "pending".to_string()
        };

        results.push(RootTaskWithStatus {
            id,
            task_type: task_type_val,
            status,
            root_id,
            crawler_id,
            image_id,
            params,
            error_message,
            retry_count,
            created_at: created_at.to_string(),
            updated_at: updated_at.to_string(),
            completed_at: completed_at.map(|dt| dt.to_string()),
            derived_status,
            children_count: children_count as u64,
        });
    }

    Ok(results)
}

/// 创建子任务，自动继承父任务的 root_id
///
/// 如果父任务有 `root_id`，子任务继承它；否则子任务的 `root_id` 设为父任务的 `id`。
/// `parent_id` 自动设为父任务的 `id`。
pub async fn create_with_parent(
    db: &DatabaseConnection,
    parent_id: &str,
    task_type: &str,
    crawler_id: Option<i32>,
    image_id: Option<i32>,
    params: Option<&str>,
) -> Result<task::Model, DbErr> {
    let parent = Task::find_by_id(parent_id.to_string())
        .one(db)
        .await?
        .ok_or(DbErr::RecordNotFound(format!(
            "父任务 {parent_id} 不存在"
        )))?;

    let effective_root_id = parent.root_id.unwrap_or_else(|| parent.id.clone());

    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();

    let model = task::ActiveModel {
        id: Set(id),
        fang_task_id: Set(None),
        task_type: Set(task_type.to_string()),
        status: Set(STATUS_PENDING.to_string()),
        parent_id: Set(Some(parent_id.to_string())),
        root_id: Set(Some(effective_root_id)),
        crawler_id: Set(crawler_id),
        image_id: Set(image_id),
        params: Set(params.map(|s| s.to_string())),
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
    statuses: &[&str],
    older_than_hours: i64,
) -> Result<u64, DbErr> {
    if statuses.is_empty() || older_than_hours <= 0 {
        return Ok(0);
    }

    let cutoff = Utc::now() - chrono::Duration::hours(older_than_hours);

    let result = Task::delete_many()
        .filter(task::Column::Status.is_in(statuses.iter().copied()))
        .filter(task::Column::CompletedAt.is_not_null())
        .filter(task::Column::CompletedAt.lt(cutoff))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}
