use chrono::Utc;
use sea_orm::*;

use crate::db::entities::pixiv_credential::{self, Entity as PixivCredential};

pub async fn create(
    db: &DatabaseConnection,
    pixiv_user_id: &str,
    refresh_token: &str,
    note: Option<&str>,
) -> Result<pixiv_credential::Model, DbErr> {
    let now = Utc::now().naive_utc();
    let model = pixiv_credential::ActiveModel {
        pixiv_user_id: Set(pixiv_user_id.to_string()),
        refresh_token: Set(refresh_token.to_string()),
        status: Set(pixiv_credential::STATUS_ACTIVE),
        note: Set(note.map(|s| s.to_string())),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    model.insert(db).await
}

pub async fn find_all(db: &DatabaseConnection) -> Result<Vec<pixiv_credential::Model>, DbErr> {
    PixivCredential::find().all(db).await
}

pub async fn find_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<pixiv_credential::Model>, DbErr> {
    PixivCredential::find_by_id(id).one(db).await
}

/// Pick one random active credential. Returns None if no active credentials exist.
pub async fn find_one_active_random(
    db: &DatabaseConnection,
) -> Result<Option<pixiv_credential::Model>, DbErr> {
    // ORDER BY RANDOM() works on both SQLite and PostgreSQL
    PixivCredential::find()
        .filter(pixiv_credential::Column::Status.eq(pixiv_credential::STATUS_ACTIVE))
        .order_by_asc(pixiv_credential::Column::Id) // stable fallback
        .all(db)
        .await
        .map(|mut rows| {
            if rows.is_empty() {
                None
            } else {
                // Manual random selection to avoid DB-dialect-specific RANDOM()
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let hash = {
                    let mut h = DefaultHasher::new();
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0).hash(&mut h);
                    h.finish()
                };
                let idx = (hash as usize) % rows.len();
                Some(rows.swap_remove(idx))
            }
        })
}

/// Partial update — only sets fields that are Some.
pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    refresh_token: Option<&str>,
    access_token: Option<Option<&str>>,
    status: Option<i32>,
    note: Option<Option<&str>>,
) -> Result<Option<pixiv_credential::Model>, DbErr> {
    let Some(model) = PixivCredential::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: pixiv_credential::ActiveModel = model.into();
    if let Some(rt) = refresh_token {
        active.refresh_token = Set(rt.to_string());
    }
    if let Some(at) = access_token {
        active.access_token = Set(at.map(|s| s.to_string()));
    }
    if let Some(s) = status {
        active.status = Set(s);
    }
    if let Some(n) = note {
        active.note = Set(n.map(|s| s.to_string()));
    }
    active.updated_at = Set(Utc::now().naive_utc());
    let updated = active.update(db).await?;
    Ok(Some(updated))
}

/// Update tokens after a successful refresh.
pub async fn update_token(
    db: &DatabaseConnection,
    id: i32,
    refresh_token: &str,
    access_token: Option<&str>,
) -> Result<(), DbErr> {
    let Some(model) = PixivCredential::find_by_id(id).one(db).await? else {
        return Ok(());
    };
    let mut active: pixiv_credential::ActiveModel = model.into();
    active.refresh_token = Set(refresh_token.to_string());
    active.access_token = Set(access_token.map(|s| s.to_string()));
    active.last_refreshed_at = Set(Some(Utc::now().naive_utc()));
    active.updated_at = Set(Utc::now().naive_utc());
    active.update(db).await?;
    Ok(())
}

/// Update status only.
pub async fn update_status(
    db: &DatabaseConnection,
    id: i32,
    status: i32,
) -> Result<(), DbErr> {
    let Some(model) = PixivCredential::find_by_id(id).one(db).await? else {
        return Ok(());
    };
    let mut active: pixiv_credential::ActiveModel = model.into();
    active.status = Set(status);
    active.updated_at = Set(Utc::now().naive_utc());
    active.update(db).await?;
    Ok(())
}

/// Update last_used_at timestamp.
pub async fn touch_last_used(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    let Some(model) = PixivCredential::find_by_id(id).one(db).await? else {
        return Ok(());
    };
    let mut active: pixiv_credential::ActiveModel = model.into();
    active.last_used_at = Set(Some(Utc::now().naive_utc()));
    active.updated_at = Set(Utc::now().naive_utc());
    active.update(db).await?;
    Ok(())
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<bool, DbErr> {
    let result = PixivCredential::delete_by_id(id).exec(db).await?;
    Ok(result.rows_affected > 0)
}
