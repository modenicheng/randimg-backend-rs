use sea_orm::entity::prelude::*;
use sea_orm::JsonValue;
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
    pub accessable: Option<bool>,
    pub uploaded: bool,
    pub downloaded: bool,
    pub processed: bool,
    pub processing: bool,
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

impl ActiveModelBehavior for ActiveModel {}
