use crate::db::entities::admin::{self, Entity as Admin};
use sea_orm::*;

pub async fn find_by_username(
    db: &DatabaseConnection,
    username: &str,
) -> Result<Option<admin::Model>, DbErr> {
    Admin::find()
        .filter(admin::Column::Username.eq(username))
        .one(db)
        .await
}

pub async fn create(
    db: &DatabaseConnection,
    username: &str,
    password_hash: &str,
    is_superuser: bool,
) -> Result<admin::Model, DbErr> {
    let model = admin::ActiveModel {
        username: Set(username.to_string()),
        password: Set(password_hash.to_string()),
        is_superuser: Set(is_superuser),
        ..Default::default()
    };
    let result = model.insert(db).await?;
    Ok(result)
}
