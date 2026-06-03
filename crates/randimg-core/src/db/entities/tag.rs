use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tags")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub translated_name: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::image_tag_association::Entity")]
    ImageTagAssociation,
}

impl Related<super::image::Entity> for Entity {
    fn to() -> RelationDef {
        super::image_tag_association::Relation::Tag.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::image_tag_association::Relation::Image.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
