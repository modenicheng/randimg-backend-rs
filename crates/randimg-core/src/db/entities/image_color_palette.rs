use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "image_color_palette")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub image_id: i32,
    pub color_index: i32,
    pub rgb_r: i32,
    pub rgb_g: i32,
    pub rgb_b: i32,
    pub lab_l: f64,
    pub lab_a: f64,
    pub lab_b: f64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::image::Entity",
        from = "Column::ImageId",
        to = "super::image::Column::Id"
    )]
    Image,
}

impl Related<super::image::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Image.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
