use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "authors")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub platform: Option<String>,
    pub platform_id: Option<String>,
    pub homepage: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::image::Entity")]
    Images,
}

impl Related<super::image::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Images.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
