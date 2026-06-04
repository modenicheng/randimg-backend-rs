use sea_orm::JsonValue;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "images")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub title: String,
    pub image_path: String,
    pub source_url: Option<String>,
    pub source_id: Option<i32>,
    pub source_image_url: Option<String>,
    pub author_id: i32,
    pub width: i32,
    pub height: i32,
    pub aspect_ratio: f32,
    pub colors: Option<JsonValue>,
    pub primary_l: Option<f64>,
    pub primary_a: Option<f64>,
    pub primary_b: Option<f64>,
    pub accessible: Option<bool>,
    pub avatar_available: Option<bool>,
    pub is_public: bool,
    pub downloaded: bool,
    pub source_created_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(column_type = "BigInteger")]
    pub total_view: i64,
    #[sea_orm(column_type = "BigInteger")]
    pub total_bookmarks: i64,
    #[sea_orm(column_type = "BigInteger")]
    pub total_comments: i64,
    pub fetched_times: i32,
    pub created_at: DateTimeWithTimeZone,
    pub deleted_at: Option<DateTimeWithTimeZone>,
    pub illust_type: Option<String>,
    #[sea_orm(default_value = 0)]
    pub x_restrict: i32,
    #[sea_orm(default_value = 0)]
    pub illust_ai_type: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::author::Entity",
        from = "Column::AuthorId",
        to = "super::author::Column::Id"
    )]
    Author,
    #[sea_orm(has_many = "super::image_tag_association::Entity")]
    ImageTagAssociation,
    #[sea_orm(has_many = "super::image_color_palette::Entity")]
    ImageColorPalette,
}

impl Related<super::author::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Author.def()
    }
}

impl Related<super::tag::Entity> for Entity {
    fn to() -> RelationDef {
        super::image_tag_association::Relation::Image.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::image_tag_association::Relation::Tag.def().rev())
    }
}

impl Related<super::image_color_palette::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ImageColorPalette.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
