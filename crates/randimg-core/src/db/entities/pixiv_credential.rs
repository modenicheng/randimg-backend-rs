use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Account status constants for future fallback / scheduling.
pub const STATUS_ACTIVE: i32 = 0;
pub const STATUS_EXPIRED: i32 = 1;
pub const STATUS_DISABLED: i32 = 2;
pub const STATUS_RATE_LIMITED: i32 = 3;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "pixiv_credentials")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub pixiv_user_id: String,
    #[serde(skip)]
    pub refresh_token: String,
    #[serde(skip)]
    pub access_token: Option<String>,
    pub status: i32,
    pub note: Option<String>,
    pub last_used_at: Option<NaiveDateTime>,
    pub last_refreshed_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
