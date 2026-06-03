/// 任务查询函数
///
/// 提供 tasks 表的 CRUD 操作，替代 apalis_job 查询模块。
/// 使用 SeaORM 查询构建器，支持 SQLite 和 PostgreSQL。
use crate::db::entities::task::{
    self, Entity as Task, STATUS_DONE, STATUS_FAILED, STATUS_KILLED, STATUS_PENDING, STATUS_QUEUED,
    STATUS_RUNNING,
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

/// 按 ID 删除任务（含 task_dependencies 清理）
pub async fn delete_by_id(db: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
    use crate::db::entities::task_dependency::{Column as DepCol, Entity as TaskDependency};

    let id_owned = id.to_string();
    let result = db.transaction::<_, bool, DbErr>(|txn| {
        Box::pin(async move {
            // 删除引用此任务作为子任务的依赖关系
            TaskDependency::delete_many()
                .filter(DepCol::ChildJobId.eq(&id_owned))
                .exec(txn)
                .await?;

            // 删除引用此任务作为父任务的依赖关系
            TaskDependency::delete_many()
                .filter(DepCol::ParentJobId.eq(&id_owned))
                .exec(txn)
                .await?;

            // 删除任务本身
            let result = Task::delete_many()
                .filter(task::Column::Id.eq(&id_owned))
                .exec(txn)
                .await?;

            Ok(result.rows_affected > 0)
        })
    })
    .await
    .map_err(|e| match e {
        TransactionError::Connection(e) => e,
        TransactionError::Transaction(e) => e,
    })?;

    Ok(result)
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

/// 按状态批量删除任务
///
/// 同时清理 task_dependencies 中的关联行，返回删除的任务数量。
pub async fn delete_by_statuses(
    db: &DatabaseConnection,
    statuses: &[&str],
    task_type: Option<&str>,
) -> Result<u64, DbErr> {
    use crate::db::entities::task_dependency::{Column as DepCol, Entity as TaskDependency};

    if statuses.is_empty() {
        return Ok(0);
    }

    // 第一步：查找匹配的任务 ID
    let mut q = Task::find()
        .select_only()
        .column(task::Column::Id)
        .filter(task::Column::Status.is_in(statuses.iter().copied()));
    if let Some(tt) = task_type {
        q = q.filter(task::Column::TaskType.eq(tt));
    }
    let ids: Vec<String> = q.into_tuple().all(db).await?;

    if ids.is_empty() {
        return Ok(0);
    }

    // 第二步：原子删除依赖关系和任务
    let ids_clone = ids.clone();
    let result = db.transaction::<_, u64, DbErr>(|txn| {
        Box::pin(async move {
            // 删除这些任务作为子任务的依赖关系
            TaskDependency::delete_many()
                .filter(DepCol::ChildJobId.is_in(&ids_clone))
                .exec(txn)
                .await?;

            // 删除这些任务作为父任务的依赖关系
            TaskDependency::delete_many()
                .filter(DepCol::ParentJobId.is_in(&ids_clone))
                .exec(txn)
                .await?;

            // 删除任务本身
            let result = Task::delete_many()
                .filter(task::Column::Id.is_in(&ids_clone))
                .exec(txn)
                .await?;

            Ok(result.rows_affected)
        })
    })
    .await
    .map_err(|e| match e {
        TransactionError::Connection(e) => e,
        TransactionError::Transaction(e) => e,
    })?;

    Ok(result)
}

/// 按状态和 ID 列表批量删除任务
///
/// 只删除同时匹配状态列表和 ID 列表的任务。
/// 同时清理 task_dependencies 行。返回删除的任务数量。
pub async fn delete_by_statuses_and_ids(
    db: &DatabaseConnection,
    statuses: &[&str],
    ids: &[String],
) -> Result<u64, DbErr> {
    use crate::db::entities::task_dependency::{Column as DepCol, Entity as TaskDependency};

    if statuses.is_empty() || ids.is_empty() {
        return Ok(0);
    }

    // 查找匹配的任务
    let to_delete: Vec<String> = Task::find()
        .select_only()
        .column(task::Column::Id)
        .filter(task::Column::Status.is_in(statuses.iter().copied()))
        .filter(task::Column::Id.is_in(ids.iter().map(|s| s.as_str())))
        .into_tuple()
        .all(db)
        .await?;

    if to_delete.is_empty() {
        return Ok(0);
    }

    let ids_clone = to_delete.clone();
    let result = db.transaction::<_, u64, DbErr>(|txn| {
        Box::pin(async move {
            TaskDependency::delete_many()
                .filter(DepCol::ChildJobId.is_in(&ids_clone))
                .exec(txn)
                .await?;

            TaskDependency::delete_many()
                .filter(DepCol::ParentJobId.is_in(&ids_clone))
                .exec(txn)
                .await?;

            let result = Task::delete_many()
                .filter(task::Column::Id.is_in(&ids_clone))
                .exec(txn)
                .await?;

            Ok(result.rows_affected)
        })
    })
    .await
    .map_err(|e| match e {
        TransactionError::Connection(e) => e,
        TransactionError::Transaction(e) => e,
    })?;

    Ok(result)
}

/// 按任务类型查找爬虫 ID 列表
///
/// 用于查询指定 crawl_type 的 crawl 任务对应的 crawler_id。
pub async fn find_crawl_ids_by_type(
    db: &DatabaseConnection,
    crawl_type: i32,
) -> Result<Vec<i32>, DbErr> {
    // params 字段存储 JSON，使用原生 SQL 解析
    #[cfg(feature = "db-sqlite")]
    {
        let sql = "SELECT crawler_id FROM tasks WHERE task_type = 'crawl' AND json_extract(params, '$.crawl_type') = ? AND crawler_id IS NOT NULL";
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

    #[cfg(feature = "db-postgres")]
    {
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
        if new_status == STATUS_DONE || new_status == STATUS_FAILED || new_status == STATUS_KILLED
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
        active.retry_count = Set(active.retry_count.unwrap() + 1);
        active.updated_at = Set(Utc::now().into());
        active.update(db).await?;
    }
    Ok(())
}

/// 创建新任务
pub async fn create(
    db: &DatabaseConnection,
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
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        completed_at: Set(None),
    };
    model.insert(db).await
}

/// 关联 Fang 任务 ID 并更新状态为 queued
pub async fn link_fang_task(
    db: &DatabaseConnection,
    task_id: &str,
    fang_task_id: i64,
) -> Result<(), DbErr> {
    if let Some(t) = Task::find_by_id(task_id.to_string()).one(db).await? {
        let mut active: task::ActiveModel = t.into();
        active.fang_task_id = Set(Some(fang_task_id));
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
pub async fn list_roots(
    db: &DatabaseConnection,
    task_type: Option<&str>,
    limit: u64,
    offset: u64,
) -> Result<Vec<RootTaskWithStatus>, DbErr> {
    let mut query = Task::find()
        .filter(task::Column::ParentId.is_null())
        .order_by_desc(task::Column::CreatedAt);

    if let Some(tt) = task_type {
        query = query.filter(task::Column::TaskType.eq(tt));
    }

    let roots = query.limit(limit).offset(offset).all(db).await?;

    let mut results = Vec::with_capacity(roots.len());
    for root in roots {
        let derived_status = compute_derived_status(db, &root.id).await?;

        let children_count = Task::find()
            .filter(task::Column::RootId.eq(&root.id))
            .filter(task::Column::Id.ne(&root.id))
            .count(db)
            .await?;

        results.push(RootTaskWithStatus {
            id: root.id,
            task_type: root.task_type,
            status: root.status,
            root_id: root.root_id,
            crawler_id: root.crawler_id,
            image_id: root.image_id,
            params: root.params,
            error_message: root.error_message,
            retry_count: root.retry_count,
            created_at: root.created_at.to_string(),
            updated_at: root.updated_at.to_string(),
            completed_at: root.completed_at.map(|dt| dt.to_string()),
            derived_status,
            children_count,
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
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        completed_at: Set(None),
    };
    model.insert(db).await
}
